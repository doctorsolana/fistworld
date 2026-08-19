//! Weather state (cloud cover, THE storm system, shared wind) — the visuals
//! live in the procedural atmosphere and the cloud plane (cloud_layer.rs).
//! The legacy textured sky dome that used to live here is retired: it was
//! nearly invisible in clear weather, carried a baked-in second sun, and only
//! smeared a gray veil over the procedural sky in cloudy weather.

use super::day_night::{lerp_f32, smoothstep};
use super::*;

const CLOUD_COVER_SEGMENT_SECS: f32 = 180.0;
const CLOUD_COVER_LERP_SPEED: f32 = 0.08;
const CLOUD_COVER_CLEAR_RANGE: (f32, f32) = (0.0, 0.15);
const CLOUD_COVER_CLOUDY_RANGE: (f32, f32) = (0.30, 0.50);
/// Base (between-cells) cover while a storm system is on the map: the drama
/// lives in the drifting cells, so the sky between them stays broken, not
/// blanketed — the world must remain readable even mid-storm.
const CLOUD_COVER_STORM_BASE: (f32, f32) = (0.18, 0.30);

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloudCoverMode {
    #[default]
    Auto,
    Clear,
    Cloudy,
    /// Dev/capture override: a full storm system overhead.
    Storm,
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudCoverOverride {
    pub mode: CloudCoverMode,
}

impl Default for CloudCoverOverride {
    fn default() -> Self {
        Self {
            mode: CloudCoverMode::Auto,
        }
    }
}

#[derive(Resource, Clone, Copy)]
pub struct CloudCover {
    pub current: f32,
    pub target: f32,
    /// Storm-system strength 0..1: gates the drifting storm cells that
    /// locally thicken cloud and rain-darken the ground.
    pub storminess: f32,
    pub storm_target: f32,
    pub segment: i64,
}

impl Default for CloudCover {
    fn default() -> Self {
        // Start near-clear: the lerp toward the rolled weather is slow
        // (~90s), and joining the world under a dissolving cloud blanket
        // read as a rendering glitch rather than weather.
        Self {
            current: 0.12,
            target: 0.12,
            storminess: 0.0,
            storm_target: 0.0,
            segment: -1,
        }
    }
}

impl CloudCover {
    /// Fully-settled state for a forced weather mode: captures and the god-mode
    /// FORCE buttons want the look NOW, not after the ~90s natural transition.
    pub fn snapped(mode: CloudCoverMode) -> Self {
        let (cover, storm) = match mode {
            CloudCoverMode::Auto => return Self::default(),
            CloudCoverMode::Clear => (0.0, 0.0),
            CloudCoverMode::Cloudy => (CLOUD_COVER_CLOUDY_RANGE.1, 0.0),
            CloudCoverMode::Storm => (CLOUD_COVER_STORM_BASE.1, 1.0),
        };
        Self {
            current: cover,
            target: cover,
            storminess: storm,
            storm_target: storm,
            segment: -1,
        }
    }
}

/// Fixed wind bearing shared by the cloud plane and the cloud shadows.
///
/// The world's ONE wind now, rather than a bearing of its own. It was
/// `(0.86, 0.5)`: 4.7 degrees off the foliage and 0.99479 long, so cloud drift
/// ran half a percent slower than `CLOUD_WIND_SPEED` claimed and the storm's
/// 400 m meander below was really 397.9 m. Both are exact now.
const CLOUD_WIND_BEARING: Vec2 = crate::wind::WIND_DIRECTION;
/// World-space cloud drift offset at an absolute world time.
///
/// THE single source of wind for the cloud plane and the terrain/water cloud
/// shadows — both must call this or they desync. The varying speed is applied
/// as its exact closed-form integral, so gusts accelerate the drift without
/// ever teleporting the field, and the result stays deterministic, identical
/// across clients, and time-warp aware (absolute world seconds in, offset out).
/// Wind offset AND instantaneous speed (world units/sec along the bearing).
///
/// The speed is what shaders extrapolate with between anchor writes: material
/// uniforms carry (offset at anchor time, speed), and `globals.time` advances
/// the drift per-frame, so cloud/shadow motion is frame-smooth while material
/// re-uploads stay at ~1/sec.
pub(super) fn cloud_wind_state(abs_seconds: f32, seed_phase: f32) -> (Vec2, f32) {
    let (offset, speed) = crate::wind::wind_state(abs_seconds, seed_phase);
    debug_assert_eq!(CLOUD_WIND_BEARING, crate::wind::WIND_DIRECTION);
    (offset, speed)
}

/// How much slower THE storm system drifts than the clouds streaming through
/// it. Must match the extrapolation factor in the shaders' storm callers.
pub(super) const STORM_DRIFT_FACTOR: f32 = 0.55;
/// The storm center is reflected inside +/- this fraction of the half extent,
/// so a storm is ALWAYS somewhere on the map — the drift is slow (~1.7 m/s),
/// and a wrapped-off-map storm would leave FORCE STORM showing nothing for
/// tens of minutes.
const STORM_TRACK_LIMIT_FRAC: f32 = 0.85;

/// Center of THE storm system — one per map, by design: a concentrated squall
/// that drifts over the land, not a field of cells. Deterministic in
/// (seed phase, absolute world seconds), so every client sees the same storm
/// in the same place. Drifts at [`STORM_DRIFT_FACTOR`]x the cloud wind plus a
/// slow crosswind meander (amplitude/rate kept small enough that the ~1 Hz
/// anchor writes never step visibly). The track REFLECTS off the map bounds
/// (triangle fold) instead of wrapping: continuous position, no teleport, and
/// the storm never leaves the playfield.
pub(super) fn storm_center(seed_phase: f32, abs_seconds: f32, half_extent: f32) -> Vec2 {
    let (wind_offset, _) = cloud_wind_state(abs_seconds, seed_phase);
    // Float-only spawn hash: derived from the same seed phase the shaders
    // already carry, so no extra plumbing.
    let h = |k: f32| ((seed_phase * k).sin() * 43_758.547).fract();
    let spawn = Vec2::new(
        (h(12.9898) - 0.5) * 1.6 * half_extent,
        (h(78.233) - 0.5) * 1.6 * half_extent,
    );
    let perp = Vec2::new(-CLOUD_WIND_BEARING.y, CLOUD_WIND_BEARING.x);
    let meander = perp * (400.0 * (abs_seconds * 0.002 + seed_phase).sin());
    let pos = spawn + wind_offset * STORM_DRIFT_FACTOR + meander;
    let limit = half_extent * STORM_TRACK_LIMIT_FRAC;
    let reflect = |v: f32| {
        let u = (v + limit).rem_euclid(4.0 * limit);
        let folded = if u <= 2.0 * limit { u } else { 4.0 * limit - u };
        folded - limit
    };
    Vec2::new(reflect(pos.x), reflect(pos.y))
}

pub(super) fn hash_to_unit(seed: u64, salt: u64) -> f32 {
    let mut x = seed ^ salt;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32
}

/// Advance the deterministic weather roll and ease cover/storminess toward it.
pub fn update_cloud_cover(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    seed_query: Query<&shared::components::CloudSeed>,
    override_mode: Res<CloudCoverOverride>,
    mut cover: ResMut<CloudCover>,
) {
    // Deliberately NOT gated on clouds_enabled: the weather state must keep
    // evolving while clouds are switched off (the GPU pushes gate the
    // visuals), or storminess freezes at its last value and the rain
    // darkening would come back stale when clouds are re-enabled.
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);

    // Absolute calendar seconds, not time-of-day: weather segments must not
    // replay the same pattern every day.
    let seconds = (world_time.day as f32 * world_time.cycle_duration()
        + world_time.seconds_in_cycle.max(0.0))
    .max(0.0);
    let segment = (seconds / CLOUD_COVER_SEGMENT_SECS).floor() as i64;
    match override_mode.mode {
        CloudCoverMode::Clear => {
            cover.segment = segment;
            cover.target = 0.0;
            cover.storm_target = 0.0;
        }
        CloudCoverMode::Cloudy => {
            cover.segment = segment;
            // Deliberately never full overcast: an RTS must keep the world
            // readable under the weather, so "cloudy" tops out below blanket.
            cover.target = CLOUD_COVER_CLOUDY_RANGE.1;
            cover.storm_target = 0.0;
        }
        CloudCoverMode::Storm => {
            cover.segment = segment;
            cover.target = CLOUD_COVER_STORM_BASE.1;
            cover.storm_target = 1.0;
        }
        CloudCoverMode::Auto => {
            if cover.segment != segment {
                cover.segment = segment;

                let elevation = -world_time.sun_phase().cos();
                let dawn_dusk = 1.0 - smoothstep(0.25, 0.6, elevation.abs());
                // Cloudy spells are a fairly rare event — the default sky is
                // the sparse "forced clear" look; weather is the exception.
                let cloudy_chance = lerp_f32(0.12, 0.22, dawn_dusk);

                let pick = hash_to_unit(seed, segment as u64);
                let roll = hash_to_unit(seed ^ 0x9E37_79B9_7F4A_7C15, segment as u64);

                if pick < cloudy_chance {
                    cover.target =
                        lerp_f32(CLOUD_COVER_CLOUDY_RANGE.0, CLOUD_COVER_CLOUDY_RANGE.1, roll);
                    // A cloudy spell sometimes carries a storm system.
                    let storm_roll = hash_to_unit(seed ^ 0x5708_11ED, segment as u64);
                    if storm_roll < 0.35 {
                        cover.storm_target =
                            0.6 + 0.4 * hash_to_unit(seed ^ 0x5708_22EE, segment as u64);
                        // Storm sky: broken cover between the cells, not a blanket.
                        cover.target =
                            lerp_f32(CLOUD_COVER_STORM_BASE.0, CLOUD_COVER_STORM_BASE.1, roll);
                    } else {
                        cover.storm_target = 0.0;
                    }
                } else {
                    cover.target =
                        lerp_f32(CLOUD_COVER_CLEAR_RANGE.0, CLOUD_COVER_CLEAR_RANGE.1, roll);
                    cover.storm_target = 0.0;
                };
            }
        }
    }

    let dt = time.delta_secs().max(0.0);
    let blend = 1.0 - (-CLOUD_COVER_LERP_SPEED * dt).exp();
    cover.current = lerp_f32(cover.current, cover.target, blend).clamp(0.0, 1.0);
    cover.storminess = lerp_f32(cover.storminess, cover.storm_target, blend).clamp(0.0, 1.0);
}

#[cfg(test)]
mod storm_tests {
    use super::*;

    /// The storm must always be ON the map (FORCE STORM shows a storm now,
    /// not in twenty minutes), and the track must be continuous — the
    /// reflect fold must never jump the center between nearby timestamps.
    #[test]
    fn storm_center_stays_on_map_and_moves_continuously() {
        let seed_phase = hash_to_unit(7, 0) * 37.0;
        let half = 4096.0;
        let mut prev = storm_center(seed_phase, 0.0, half);
        println!(
            "capture-seed storm center @225s: {:?}",
            storm_center(seed_phase, 225.0, half)
        );
        for i in 1..20_000 {
            let t = i as f32 * 1.0;
            let c = storm_center(seed_phase, t, half);
            assert!(
                c.x.abs() <= half && c.y.abs() <= half,
                "off map at {t}: {c:?}"
            );
            assert!(c.distance(prev) < 8.0, "teleport at {t}: {prev:?} -> {c:?}");
            prev = c;
        }
    }
}
