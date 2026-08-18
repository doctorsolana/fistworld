//! Deterministic, server-authoritative wind shared by simulation and visuals.

use bevy::prelude::*;

/// Prevailing downwind direction in world XZ.
pub const WIND_DIRECTION: Vec2 = Vec2::new(0.8206, 0.5715);
/// Exact shader literal mirrored by the client asset validation tests.
pub const WIND_DIRECTION_WGSL: &str = "vec2<f32>(0.8206, 0.5715)";
pub const WIND_SPEED_MIN: f32 = 1.5;
pub const WIND_SPEED_MAX: f32 = 4.5;
pub const GUST_TIME_SCALE: f32 = 1.15;

/// Slowly wandering local downwind bearing at absolute world time.
pub fn wind_direction(abs_seconds: f32, seed_phase: f32) -> Vec2 {
    use std::f32::consts::TAU;

    let prevailing_bearing = WIND_DIRECTION.x.atan2(WIND_DIRECTION.y);
    let primary = (TAU * abs_seconds / 1_800.0 + seed_phase * 0.37).sin() * 0.72;
    let secondary = (TAU * abs_seconds / 617.0 + seed_phase * 1.91).sin() * 0.24;
    let bearing = prevailing_bearing + primary + secondary;
    Vec2::new(bearing.sin(), bearing.cos())
}

pub fn wind_seed_phase(seed: u64) -> f32 {
    let mut x = seed;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32 * 37.0
}

/// Integrated prevailing offset and instantaneous wind speed.
pub fn wind_state(abs_seconds: f32, seed_phase: f32) -> (Vec2, f32) {
    use std::f32::consts::TAU;

    let amplitude = 0.5 * (WIND_SPEED_MAX - WIND_SPEED_MIN);
    let midpoint = WIND_SPEED_MIN + amplitude;
    let primary_rate = TAU / 540.0;
    let secondary_rate = TAU / 197.0;
    let primary_phase = seed_phase;
    let secondary_phase = seed_phase * 2.7;
    let integral = midpoint * abs_seconds
        + amplitude
            * (0.7 * (primary_phase.cos() - (primary_rate * abs_seconds + primary_phase).cos())
                / primary_rate
                + 0.3
                    * (secondary_phase.cos()
                        - (secondary_rate * abs_seconds + secondary_phase).cos())
                    / secondary_rate);
    let speed = midpoint
        + amplitude
            * (0.7 * (primary_rate * abs_seconds + primary_phase).sin()
                + 0.3 * (secondary_rate * abs_seconds + secondary_phase).sin());
    (WIND_DIRECTION * integral, speed)
}

/// Playable sailing multiplier from heading and wind. A vessel always retains
/// steerage, reaches its best speed on a broad reach, and is slowest directly
/// into the wind. Ship classes can multiply their own hull speed by this.
pub fn sailing_speed_multiplier(forward: Vec2, downwind: Vec2, wind_speed: f32) -> f32 {
    let forward = forward.normalize_or_zero();
    let downwind = downwind.normalize_or_zero();
    if forward == Vec2::ZERO || downwind == Vec2::ZERO {
        return 0.65;
    }
    let alignment = forward.dot(downwind).clamp(-1.0, 1.0);
    // Downwind is useful, upwind retains minimum steerage, and a broad reach
    // receives the largest bonus. This is intentionally forgiving rather than
    // a sailing simulator that can trap a new player offshore.
    let broad_reach = (1.0 - alignment * alignment).sqrt();
    let bearing_factor = 0.52 + alignment.max(0.0) * 0.20 + broad_reach * 0.38;
    let strength = (wind_speed / ((WIND_SPEED_MIN + WIND_SPEED_MAX) * 0.5)).clamp(0.55, 1.45);
    (bearing_factor * strength).clamp(0.38, 1.35)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broad_reach_is_faster_than_sailing_into_wind() {
        let wind = Vec2::Y;
        let into_wind = sailing_speed_multiplier(Vec2::NEG_Y, wind, 3.0);
        let broad = sailing_speed_multiplier(Vec2::X, wind, 3.0);
        assert!(broad > into_wind);
        assert!(into_wind > 0.0);
    }
}
