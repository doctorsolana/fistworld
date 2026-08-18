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
    /// Start at 07:30 on the display clock: the sun is about 17 degrees above
    /// the horizon, giving the first player a warm sunrise that is still bright
    /// enough to read the character, sail and coastline clearly.
    pub const DEFAULT_START_SECONDS_IN_DAY: f32 = 135.0;
    /// Displayed clock hour of sunrise. The display clock is deliberately
    /// asymmetric (long summer days): daylight owns 06:00-22:00 so dusk lands
    /// late in the evening instead of mid-afternoon.
    pub const SUNRISE_NORMALIZED: f32 = 6.0 / 24.0;
    /// Displayed clock hour of sunset (22:00).
    pub const SUNSET_NORMALIZED: f32 = 22.0 / 24.0;
    /// Ordinary production shifts end three quarters through daylight (about
    /// 18:00 on the deliberately long summer display day).
    pub const WORKDAY_END_DAY_T: f32 = 0.75;

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

    /// Whether ordinary private production is currently on shift.
    pub fn is_ordinary_work_time(&self) -> bool {
        self.is_day() && self.day_t() < Self::WORKDAY_END_DAY_T
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

    /// DISPLAY clock, normalized 0..1 of a 24h day (0 = midnight). Sunrise
    /// displays at [`Self::SUNRISE_NORMALIZED`] (06:00) and sunset at
    /// [`Self::SUNSET_NORMALIZED`] (20:00) — summer hours, so dusk lands at a
    /// late clock time. This is presentation ONLY: anything driving light or
    /// simulation from the sun must use [`Self::sun_phase`], never this.
    pub fn normalized_time(&self) -> f32 {
        let day_span = Self::SUNSET_NORMALIZED - Self::SUNRISE_NORMALIZED;
        let night_span = 1.0 - day_span;
        if self.is_day() {
            Self::SUNRISE_NORMALIZED + self.day_t() * day_span
        } else {
            (Self::SUNSET_NORMALIZED + self.night_t() * night_span).rem_euclid(1.0)
        }
    }

    /// Sun position phase for lighting: elevation == `-cos(sun_phase())`,
    /// exactly the old `normalized * TAU` convention (PI/2 = sunrise on the
    /// horizon, PI = solar noon, 3*PI/2 = sunset). Derived from cycle
    /// fractions, so it is independent of the asymmetric display clock.
    pub fn sun_phase(&self) -> f32 {
        use std::f32::consts::PI;
        if self.is_day() {
            PI * 0.5 + self.day_t() * PI
        } else {
            PI * 1.5 + self.night_t() * PI
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
        let day_span = Self::SUNSET_NORMALIZED - Self::SUNRISE_NORMALIZED;
        let night_span = 1.0 - day_span;
        let day_fraction = self.day_duration / cycle;
        let night_fraction = self.night_duration / cycle;

        let cycle_pos = if (Self::SUNRISE_NORMALIZED..Self::SUNSET_NORMALIZED).contains(&n) {
            let day_progress = (n - Self::SUNRISE_NORMALIZED) / day_span;
            day_progress * day_fraction
        } else {
            let night_progress = if n >= Self::SUNSET_NORMALIZED {
                (n - Self::SUNSET_NORMALIZED) / night_span
            } else {
                (n + 1.0 - Self::SUNSET_NORMALIZED) / night_span
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
        let wt = WorldTime::new_default();
        assert_eq!(wt.day, 0);
        assert!((wt.normalized_time() - 7.5 / 24.0).abs() < 1.0e-4);
        assert!(
            -wt.sun_phase().cos() > 0.25,
            "the default sunrise should already light the opening voyage"
        );
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
    fn display_clock_runs_summer_hours() {
        let mut wt = WorldTime::new(240.0, 40.0, 0.0);
        // Sunrise displays 06:00 no matter the real durations.
        assert!((wt.normalized_time() - WorldTime::SUNRISE_NORMALIZED).abs() < 1e-4);
        // The instant before night starts displays 20:00...
        wt.seconds_in_cycle = 239.9;
        assert!((wt.normalized_time() - WorldTime::SUNSET_NORMALIZED).abs() < 2e-3);
        // ...while the sun's PHYSICAL phase still hits solar noon mid-day.
        wt.seconds_in_cycle = 120.0;
        assert!((wt.sun_phase() - std::f32::consts::PI).abs() < 1e-3);
        assert!((-wt.sun_phase().cos() - 1.0).abs() < 1e-3);
        // Midnight (display 0.0) round-trips through the inverse mapping.
        wt.set_normalized_time(0.0);
        assert!(!wt.is_day());
        assert!((wt.normalized_time() - 0.0).rem_euclid(1.0) < 2e-3);
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
