use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Server-authoritative day/night clock replicated to clients.
///
/// The server advances `seconds_in_cycle` every fixed tick and clients use it for lighting.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WorldTime {
    /// Current time within the full day+night cycle.
    pub seconds_in_cycle: f32,
    /// Duration of the "day" portion in seconds (includes sunrise + sunset).
    pub day_duration: f32,
    /// Duration of the "night" portion in seconds.
    pub night_duration: f32,
    /// Independent looping clock for deterministic water motion.
    pub ocean_seconds: f32,
    /// World calendar: how many full cycles have elapsed since the world began.
    ///
    /// Starts at day 0 and only ever counts up — debug jumps within a cycle
    /// (`set_normalized_time`) reposition the clock without touching the calendar.
    pub day: u32,
}

/// Simulation speed multiplier, on the same singleton entity as [`WorldTime`].
///
/// Server-authoritative and mutated only through dev commands; replicated so every
/// client's HUD can show the current speed. 1.0 = real time, 0.0 = paused. Warping
/// scales both the day/night clock and the strategic tick so the world stays
/// internally consistent while fast-forwarded.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TimeWarp(pub f32);

impl TimeWarp {
    /// Upper bound keeps a warped strategic tick from starving the fixed schedule.
    pub const MAX_FACTOR: f32 = 1000.0;
    /// Smallest running speed: below this a per-tick dt (warp/60) is small enough to be
    /// absorbed by f32 addition into `seconds_in_cycle`, silently freezing the clock.
    /// Exact 0 stays valid as an explicit pause.
    pub const MIN_FACTOR: f32 = 0.1;

    pub fn clamped(factor: f32) -> Self {
        if !factor.is_finite() {
            return Self(1.0);
        }
        if factor <= 0.0 {
            return Self(0.0);
        }
        Self(factor.clamp(Self::MIN_FACTOR, Self::MAX_FACTOR))
    }
}

impl Default for TimeWarp {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Server-authoritative seed for sky/cloud generation (replicated to clients).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CloudSeed {
    pub seed: u64,
}

/// Server-authoritative active map metadata (replicated).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ActiveMapState {
    pub map_id: String,
    pub bounds_min: Vec2,
    pub bounds_max: Vec2,
    pub content_hash: u64,
}

impl WorldTime {
    /// 24 minutes of daylight: afternoons linger, so the sun sets a full four
    /// real minutes later than the old 20-minute day.
    pub const DEFAULT_DAY_DURATION: f32 = 24.0 * 60.0;
    /// 4 minutes of night — a moonlit interlude, not a second shift.
    pub const DEFAULT_NIGHT_DURATION: f32 = 4.0 * 60.0;
    /// Start early morning (near sunrise).
    pub const DEFAULT_START_SECONDS_IN_DAY: f32 = 30.0;

    pub fn new(day_duration: f32, night_duration: f32, seconds_in_cycle: f32) -> Self {
        let mut wt = Self {
            seconds_in_cycle,
            day_duration,
            night_duration,
            ocean_seconds: 0.0,
            day: 0,
        };
        wt.wrap();
        wt
    }

    pub fn new_default() -> Self {
        Self::new(
            Self::DEFAULT_DAY_DURATION,
            Self::DEFAULT_NIGHT_DURATION,
            Self::DEFAULT_START_SECONDS_IN_DAY,
        )
    }

    pub fn cycle_duration(&self) -> f32 {
        self.day_duration + self.night_duration
    }

    pub fn is_day(&self) -> bool {
        self.seconds_in_cycle < self.day_duration
    }

    pub fn day_t(&self) -> f32 {
        if self.day_duration <= 0.0 {
            return 0.0;
        }
        (self.seconds_in_cycle / self.day_duration).clamp(0.0, 1.0)
    }

    pub fn night_t(&self) -> f32 {
        if self.night_duration <= 0.0 {
            return 0.0;
        }
        ((self.seconds_in_cycle - self.day_duration) / self.night_duration).clamp(0.0, 1.0)
    }

    /// `world_dt` drives the day/night clock and calendar (warp-scaled by the server).
    /// `real_dt` drives `ocean_seconds`, which must stay wall-clock: clients latch its
    /// phase once per replicated entity and then advance their shader clocks locally at
    /// 1x, so a warped ocean clock would permanently desync wave/wet-sand phase.
    pub fn advance(&mut self, world_dt: f32, real_dt: f32) {
        let world_dt = world_dt.max(0.0);
        self.seconds_in_cycle += world_dt;
        self.ocean_seconds =
            crate::water::advance_ocean_seconds(self.ocean_seconds, real_dt.max(0.0));
        // Only real elapsed time turns the calendar; wrap() alone stays day-neutral so
        // debug clock jumps can't fabricate history.
        let cycle = self.cycle_duration();
        if cycle > 0.0 && self.seconds_in_cycle >= cycle {
            self.day = self
                .day
                .saturating_add((self.seconds_in_cycle / cycle) as u32);
        }
        self.wrap();
    }

    fn wrap(&mut self) {
        let cycle = self.cycle_duration();
        if cycle > 0.0 {
            self.seconds_in_cycle = self.seconds_in_cycle.rem_euclid(cycle);
        } else {
            self.seconds_in_cycle = 0.0;
        }
    }

    /// Returns normalized time 0.0-1.0 where:
    /// - 0.0 = midnight
    /// - 0.25 = sunrise (start of day)
    /// - 0.5 = noon (middle of day)
    /// - 0.75 = sunset (end of day, start of night)
    /// - 1.0 = back to midnight
    ///
    /// Our internal representation has day first (0 to day_duration) then night.
    /// This maps it to a more intuitive 24-hour cycle.
    pub fn normalized_time(&self) -> f32 {
        let cycle = self.cycle_duration();
        if cycle <= 0.0 {
            return 0.5; // Default to noon if misconfigured
        }

        // Fraction of day portion (sunrise to sunset)
        let day_fraction = self.day_duration / cycle; // e.g., 20/27 ≈ 0.74
                                                      // Fraction of night portion
        let night_fraction = self.night_duration / cycle; // e.g., 7/27 ≈ 0.26

        // Current position in cycle (0 to 1)
        let cycle_pos = self.seconds_in_cycle / cycle;

        if cycle_pos < day_fraction {
            // We're in the day portion (0 to day_duration maps to sunrise->sunset = 0.25 to 0.75)
            let day_progress = cycle_pos / day_fraction; // 0 to 1 within day
            0.25 + day_progress * 0.5 // Maps to 0.25 to 0.75
        } else {
            // We're in the night portion (day_duration to cycle_end maps to sunset->sunrise = 0.75 to 1.25, wrapped)
            let night_progress = (cycle_pos - day_fraction) / night_fraction; // 0 to 1 within night
                                                                              // First half of night: 0.75 to 1.0 (evening to midnight)
                                                                              // Second half of night: 0.0 to 0.25 (midnight to sunrise)
            let night_time = 0.75 + night_progress * 0.5;
            if night_time >= 1.0 {
                night_time - 1.0
            } else {
                night_time
            }
        }
    }

    /// Set the world clock using a normalized time (0.0-1.0).
    ///
    /// Deliberately leaves `day` untouched: this is a debug reposition within the
    /// current day, not the passage of time.
    pub fn set_normalized_time(&mut self, normalized: f32) {
        let cycle = self.cycle_duration();
        if cycle <= 0.0 {
            return;
        }

        let n = normalized.rem_euclid(1.0);
        let day_fraction = self.day_duration / cycle;
        let night_fraction = self.night_duration / cycle;

        let cycle_pos = if (0.25..0.75).contains(&n) {
            let day_progress = (n - 0.25) / 0.5; // 0..1
            day_progress * day_fraction
        } else {
            let night_progress = if n >= 0.75 {
                (n - 0.75) / 0.5
            } else {
                (n + 0.25) / 0.5
            };
            day_fraction + night_progress * night_fraction
        };

        self.seconds_in_cycle = cycle_pos * cycle;
        self.wrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_starts_on_day_zero() {
        assert_eq!(WorldTime::new_default().day, 0);
    }

    #[test]
    fn advancing_past_cycle_end_turns_the_calendar() {
        let mut wt = WorldTime::new(100.0, 20.0, 0.0);

        wt.advance(119.0, 119.0);
        assert_eq!(wt.day, 0);

        wt.advance(2.0, 2.0);
        assert_eq!(wt.day, 1);
        assert!((wt.seconds_in_cycle - 1.0).abs() < 1e-3);

        // One warped step spanning several cycles must count every one of them.
        wt.advance(360.0, 360.0);
        assert_eq!(wt.day, 4);
    }

    #[test]
    fn debug_clock_jumps_do_not_change_the_day() {
        let mut wt = WorldTime::new(100.0, 20.0, 90.0);
        wt.advance(40.0, 40.0);
        assert_eq!(wt.day, 1);

        wt.set_normalized_time(0.1);
        wt.set_normalized_time(0.9);
        assert_eq!(wt.day, 1);
    }

    #[test]
    fn ocean_clock_ignores_the_warped_delta() {
        let mut warped = WorldTime::new(100.0, 20.0, 0.0);
        let mut real = WorldTime::new(100.0, 20.0, 0.0);

        warped.advance(1000.0, 1.0);
        real.advance(1.0, 1.0);

        assert_eq!(warped.ocean_seconds, real.ocean_seconds);

        // Pause: the calendar freezes, the ocean keeps wall-clock time.
        let before = warped.seconds_in_cycle;
        warped.advance(0.0, 5.0);
        assert_eq!(warped.seconds_in_cycle, before);
        assert!(warped.ocean_seconds > real.ocean_seconds);
    }

    #[test]
    fn warp_clamping_pins_pause_and_floors_tiny_factors() {
        assert_eq!(TimeWarp::clamped(0.0).0, 0.0);
        assert_eq!(TimeWarp::clamped(-5.0).0, 0.0);
        assert_eq!(TimeWarp::clamped(0.01).0, TimeWarp::MIN_FACTOR);
        assert_eq!(TimeWarp::clamped(1.0).0, 1.0);
        assert_eq!(TimeWarp::clamped(5000.0).0, TimeWarp::MAX_FACTOR);
        assert_eq!(TimeWarp::clamped(f32::NAN).0, 1.0);
        assert_eq!(TimeWarp::clamped(f32::INFINITY).0, 1.0);
    }
}
