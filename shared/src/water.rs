//! Shared broad-water motion used by rendering and server-side buoyancy.

use std::f32::consts::TAU;

/// The water mesh sits slightly above the authored water plane to avoid z-fighting.
pub const WATER_SURFACE_OFFSET: f32 = 0.02;
/// Terrain depth represented by a fully deep water-mesh vertex.
pub const WATER_DEPTH_FADE_METERS: f32 = 2.5;
/// Maximum displacement from the broad offshore swell. 0.7m of heave gives
/// the open sea a Valheim-like roll the boat visibly rides; per-wave slopes
/// stay gentle because the dominant wavelength grew with the amplitude.
pub const WATER_DEEP_SWELL_AMPLITUDE: f32 = 0.70;
/// Normalized depth where broad motion begins to appear.
pub const WATER_SWELL_DEPTH_START: f32 = 0.02;
/// Normalized depth where broad motion reaches full strength.
pub const WATER_SWELL_DEPTH_FULL: f32 = 0.72;
/// The wave clock loops cleanly because each wave uses an integer harmonic of it.
pub const OCEAN_LOOP_SECONDS: f32 = 120.0;

const DIR_A_X: f32 = 0.894_427_2;
const DIR_A_Z: f32 = 0.447_213_6;
const DIR_B_X: f32 = -0.393_919_3;
const DIR_B_Z: f32 = 0.919_145;
const DIR_C_X: f32 = 0.196_116_1;
const DIR_C_Z: f32 = -0.980_580_7;

/// Stokes-style second-harmonic crest sharpening for the two dominant swells.
/// `sin(p) - e*cos(2p)` raises and narrows crests while widening troughs —
/// the trochoid silhouette — as a pure HEIGHT function: no horizontal
/// displacement, so CPU height sampling (boat buoyancy) stays trivial and the
/// 120s loop is preserved (2x an integer harmonic is still an integer
/// harmonic). Each component divides by (1 + e) so |profile| <= 1 and the
/// declared WATER_DEEP_SWELL_AMPLITUDE bound still holds.
pub const WATER_SWELL_SHARPNESS_A: f32 = 0.24;
pub const WATER_SWELL_SHARPNESS_B: f32 = 0.18;

#[inline]
fn sharp_sin(phase: f32, sharpness: f32) -> f32 {
    (phase.sin() - sharpness * (2.0 * phase).cos()) / (1.0 + sharpness)
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Advances and wraps the authoritative ocean clock without introducing a
/// wave discontinuity at the loop boundary.
#[inline]
pub fn advance_ocean_seconds(current: f32, dt: f32) -> f32 {
    (current + dt.max(0.0)).rem_euclid(OCEAN_LOOP_SECONDS)
}

/// Unit-amplitude layered swell. Keep the matching shader implementation in
/// `client/assets/toon_water.wgsl` in sync when tuning this function.
#[inline]
pub fn water_swell_unit(world_x: f32, world_z: f32, ocean_seconds: f32) -> f32 {
    // 64m primary: a long rolling swell rather than pond chop. Slopes stay
    // close to the old 42m/0.32 tuning because the amplitude grew with the
    // wavelength.
    let k_a = TAU / 64.0;
    let k_b = TAU / 24.0;
    let k_c = TAU / 13.0;
    let base_omega = TAU / OCEAN_LOOP_SECONDS;

    // Integer harmonics of the 120s loop chosen near true deep-water
    // dispersion (T ~ sqrt(2*pi*wavelength/g)): the 13m chop bobs every ~3s
    // and the 24m sea every ~4s, which is what makes the surface visibly
    // rise and fall up close instead of undulating in slow motion.
    let phase_a = (world_x * DIR_A_X + world_z * DIR_A_Z) * k_a + ocean_seconds * base_omega * 14.0;
    let phase_b = (world_x * DIR_B_X + world_z * DIR_B_Z) * k_b - ocean_seconds * base_omega * 31.0;
    let phase_c = (world_x * DIR_C_X + world_z * DIR_C_Z) * k_c + ocean_seconds * base_omega * 42.0;

    sharp_sin(phase_a, WATER_SWELL_SHARPNESS_A) * 0.52
        + sharp_sin(phase_b, WATER_SWELL_SHARPNESS_B) * 0.31
        + phase_c.sin() * 0.17
}

/// Broad animated surface displacement for physics. Rendering additionally
/// attenuates this by horizontal distance to shore and adds a tiny shore lap.
#[inline]
pub fn water_swell_height(
    world_x: f32,
    world_z: f32,
    terrain_depth: f32,
    ocean_seconds: f32,
) -> f32 {
    let depth_norm = (terrain_depth.max(0.0) / WATER_DEPTH_FADE_METERS).clamp(0.0, 1.0);
    let depth_scale = smoothstep(WATER_SWELL_DEPTH_START, WATER_SWELL_DEPTH_FULL, depth_norm);
    water_swell_unit(world_x, world_z, ocean_seconds) * WATER_DEEP_SWELL_AMPLITUDE * depth_scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swell_is_nearly_flat_at_the_waterline_and_stronger_at_depth() {
        let shallow = water_swell_height(17.0, -9.0, 0.02, 8.0).abs();
        let deep = water_swell_height(17.0, -9.0, 3.0, 8.0).abs();

        assert!(shallow < 0.002);
        assert!(deep > shallow);
        assert!(deep <= WATER_DEEP_SWELL_AMPLITUDE);
    }

    #[test]
    fn swell_clock_loops_without_a_height_jump() {
        let before = water_swell_unit(31.0, 47.0, 0.0);
        let after = water_swell_unit(31.0, 47.0, OCEAN_LOOP_SECONDS);

        assert!((before - after).abs() < 1.0e-5);
        assert!((advance_ocean_seconds(119.75, 0.5) - 0.25).abs() < 1.0e-6);
    }
}
