//! One wind, for everything the world does with it.
//!
//! Before this, the prevailing wind was written out by hand in eight places
//! with **three different directions**: the foliage gust ran at 34.85°, the
//! clouds, cloud shadows, storm cells and rain at 30.18°, and the water
//! caustics at 27.10°. Rain therefore fell four degrees off the way the grass
//! leaned, and nobody had to look at a wind vane to see it — rain and grass
//! share a frame.
//!
//! That is what happens to a value with no home. It is copied, it is rounded
//! differently each time, and the copies drift.
//!
//! # Why a mirrored literal rather than a uniform
//!
//! The obvious fix is to push the direction in from Rust. It cannot be done:
//! `bevy_shader`'s `ShaderDefVal` is `Bool | Int | UInt` — there is no float
//! variant — so `0.8206` cannot travel through a shader def. The alternatives
//! are a new uniform binding per material (a real cost, and `TerrainMaterial`
//! has already hit Metal's vertex-buffer ceiling once) or a literal in the WGSL
//! text. A literal costs nothing at runtime: it folds at compile time exactly
//! as the hand-written numbers did.
//!
//! So the literal stays, and [`tests::wind_direction_is_identical_everywhere`]
//! is what stops it drifting again. That test is the whole point of this
//! module; without it this is just a ninth copy.

use bevy::prelude::*;

/// The prevailing wind, as a UNIT vector in world XZ.
///
/// 34.85°, taken from the foliage gust because that was the only one of the
/// three already exactly unit length — and because the gust field is the thing
/// built most deliberately around it.
///
/// Normalising has two real consequences, both corrections rather than
/// regressions: the cloud bearing used to be 0.99479 long, so cloud drift ran
/// 0.52% slower than its own speed constant claimed, and the storm's 400 m
/// cross-track meander was really 397.9 m. Both now mean what they say.
pub const WIND_DIRECTION: Vec2 = Vec2::new(0.8206, 0.5715);

/// The exact text every shader must contain for [`WIND_DIRECTION`].
///
/// Compared against the shader sources by the test below. Kept as a string
/// rather than formatted from the `Vec2` because float formatting is not
/// stable enough to reproduce a source literal byte for byte, and a test that
/// fails on `0.86` vs `0.860` is a test people learn to ignore.
pub const WIND_DIRECTION_WGSL: &str = "vec2<f32>(0.8206, 0.5715)";

/// Wind speed wanders between these bounds (world metres/sec) on slow swells.
/// Clouds and mechanical wind-driven props consume the same value so a mill
/// never races while the visible weather is calm.
pub const WIND_SPEED_MIN: f32 = 1.5;
pub const WIND_SPEED_MAX: f32 = 4.5;

/// The local wind bearing at an absolute world time.
///
/// Large shader-driven surfaces retain [`WIND_DIRECTION`] as their prevailing
/// bearing (see the module-level note about uniforms), while articulated
/// objects such as a windmill can cheaply follow these slower weather shifts.
/// The two deterministic swells keep the wind from oscillating like a clock
/// while limiting it to roughly 55 degrees either side of the prevailing
/// bearing. `Vec2` is world XZ, pointing downwind.
pub fn wind_direction(abs_seconds: f32, seed_phase: f32) -> Vec2 {
    use std::f32::consts::TAU;

    let prevailing_bearing = WIND_DIRECTION.x.atan2(WIND_DIRECTION.y);
    let primary = (TAU * abs_seconds / 1_800.0 + seed_phase * 0.37).sin() * 0.72;
    let secondary = (TAU * abs_seconds / 617.0 + seed_phase * 1.91).sin() * 0.24;
    let bearing = prevailing_bearing + primary + secondary;
    Vec2::new(bearing.sin(), bearing.cos())
}

/// Derive the deterministic wind phase from the server-authoritative cloud
/// seed. Kept here so every consumer observes the same gust at the same world
/// time without replicating another continuously changing value.
pub fn wind_seed_phase(seed: u64) -> f32 {
    let mut x = seed;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32 * 37.0
}

/// Integrated downwind offset and instantaneous speed at an absolute world
/// time. The closed form is frame-rate independent and continuous through
/// time-warp changes.
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

/// How fast gust fronts sweep downwind, as a multiplier on world time.
///
/// ONE value for every plant. It used to be per-kind — grass 1.4, bushes 1.25,
/// trees 1.05 — and because it scales `t` inside the travelling wave, it scaled
/// the *propagation speed of the front*, not just how fast a plant wobbled.
/// Grass fronts ran at 22.2 m/s and tree fronts at 16.7 m/s, so the two slid
/// through a complete cycle relative to each other every 13.3 seconds: in step,
/// then in opposition six seconds later, forever.
///
/// 1.15 sits between the old extremes, putting the front at ~18 m/s.
pub const GUST_TIME_SCALE: f32 = 1.15;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every shader that moves something in the wind, and how many times each
    /// names the direction.
    ///
    /// EXACT counts, not "at least one". A presence check passes while somebody
    /// edits two of `terrain_splat`'s three sites and leaves the third behind,
    /// which is the same class of half-finished edit this module exists to
    /// catch. Paths are listed one by one because `toon_water.wgsl` sits
    /// outside `assets/shaders/` — a test that walked that folder would report
    /// green while the water drifted away from the land.
    const SHADERS: &[(&str, usize)] = &[
        ("assets/shaders/wind_foliage.wgsl", 1),
        ("assets/shaders/cloud_layer.wgsl", 2),
        ("assets/shaders/terrain_splat.wgsl", 3),
        ("assets/toon_water.wgsl", 2),
    ];

    /// Directions that used to be here. None may come back.
    const RETIRED: &[&str] = &["vec2<f32>(0.86, 0.5)", "vec2<f32>(0.8206, 0.5715f)"];

    fn shader(rel: &str) -> String {
        // CARGO_MANIFEST_DIR, not the cwd: `cargo test -p client` happens to run
        // from `client/`, but running the test binary directly does not.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
    }

    #[test]
    fn wind_direction_is_identical_everywhere() {
        for (rel, expected) in SHADERS {
            let src = shader(rel);
            let found = src.matches(WIND_DIRECTION_WGSL).count();
            assert_eq!(
                found, *expected,
                "{rel} names the wind direction {found} times, expected {expected}. \
                 If a site was added or removed, update SHADERS deliberately — do \
                 not just make the number match."
            );
        }
    }

    #[test]
    fn no_retired_wind_direction_survives() {
        for (rel, _) in SHADERS {
            let src = shader(rel);
            for old in RETIRED {
                assert!(
                    !src.contains(old),
                    "{rel} still contains the retired direction {old}; every wind \
                     vector must be WIND_DIRECTION"
                );
            }
        }
    }

    /// The literal and the Rust value must be the same numbers.
    ///
    /// Guards the case where someone fixes the shaders and forgets Rust, which
    /// the count test above cannot see.
    #[test]
    fn the_literal_matches_the_rust_constant() {
        let inner = WIND_DIRECTION_WGSL
            .trim_start_matches("vec2<f32>(")
            .trim_end_matches(')');
        let (x, y) = inner.split_once(',').expect("literal is a pair");
        assert_eq!(x.trim().parse::<f32>().unwrap(), WIND_DIRECTION.x);
        assert_eq!(y.trim().parse::<f32>().unwrap(), WIND_DIRECTION.y);
    }

    /// It must be a unit vector, or "direction" and "speed" are entangled and
    /// every speed constant downstream is quietly wrong by the shortfall.
    #[test]
    fn the_wind_direction_is_normalised() {
        let length = WIND_DIRECTION.length();
        assert!(
            (length - 1.0).abs() < 1e-4,
            "WIND_DIRECTION must be unit length, got {length}"
        );
    }

    #[test]
    fn wind_speed_stays_inside_its_declared_range() {
        let phase = wind_seed_phase(0x1234_5678);
        for second in (0..10_000).step_by(17) {
            let (_, speed) = wind_state(second as f32, phase);
            assert!(
                (WIND_SPEED_MIN..=WIND_SPEED_MAX).contains(&speed),
                "wind speed {speed} escaped {WIND_SPEED_MIN}..={WIND_SPEED_MAX}"
            );
        }
    }

    #[test]
    fn local_wind_bearing_changes_slowly_and_stays_normalised() {
        let phase = wind_seed_phase(0x1234_5678);
        let start = wind_direction(0.0, phase);
        let later = wind_direction(450.0, phase);
        assert!(
            start.distance(later) > 0.05,
            "the local bearing should visibly change during a world day"
        );
        for second in (0..10_000).step_by(17) {
            let direction = wind_direction(second as f32, phase);
            assert!((direction.length() - 1.0).abs() < 1e-5);
        }
    }
}
