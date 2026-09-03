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
    /// The display clock is deliberately literal: at 1x, one simulation second
    /// advances it by one world minute and a complete day takes 24 real minutes.
    pub const CLOCK_MINUTES_PER_DAY: f32 = 24.0 * 60.0;
    pub const SUNRISE_CLOCK_MINUTE: f32 = 5.0 * 60.0;
    pub const SUNSET_CLOCK_MINUTE: f32 = 23.0 * 60.0;
    pub const WORKDAY_START_CLOCK_MINUTE: f32 = 6.0 * 60.0;
    pub const WORKDAY_END_CLOCK_MINUTE: f32 = 18.0 * 60.0;

    /// The sun spends 18 world hours above the horizon. This controls the
    /// daylight arc only; [`Self::normalized_time`] remains linear.
    pub const DEFAULT_DAY_DURATION: f32 = Self::SUNSET_CLOCK_MINUTE - Self::SUNRISE_CLOCK_MINUTE;
    /// The six-hour night is the faster, below-horizon part of the sun's arc.
    pub const DEFAULT_NIGHT_DURATION: f32 =
        Self::CLOCK_MINUTES_PER_DAY - Self::DEFAULT_DAY_DURATION;
    /// Default to 07:30 on the display clock: the sun is comfortably above the
    /// horizon, giving simulations a warm morning that is still bright enough
    /// to read the character, sail and coastline clearly.
    pub const DEFAULT_START_SECONDS_IN_DAY: f32 = 7.5 * 60.0 - Self::SUNRISE_CLOCK_MINUTE;
    pub const SUNRISE_NORMALIZED: f32 = Self::SUNRISE_CLOCK_MINUTE / Self::CLOCK_MINUTES_PER_DAY;
    pub const SUNSET_NORMALIZED: f32 = Self::SUNSET_CLOCK_MINUTE / Self::CLOCK_MINUTES_PER_DAY;
    pub const DEFAULT_ORDINARY_SHIFT_SECONDS: f32 =
        Self::WORKDAY_END_CLOCK_MINUTE - Self::WORKDAY_START_CLOCK_MINUTE;

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
        let seconds = self.seconds_in_cycle;
        seconds >= self.ordinary_work_start_seconds() && seconds < self.ordinary_work_end_seconds()
    }

    /// Convert a displayed-clock offset from sunrise into this clock's internal
    /// cycle seconds. Custom short-cycle test clocks therefore keep the same
    /// clock-hour semantics without assuming the production defaults.
    fn cycle_seconds_for_clock_minutes(&self, minutes: f32) -> f32 {
        self.cycle_duration() * minutes / Self::CLOCK_MINUTES_PER_DAY
    }

    pub fn ordinary_work_start_seconds(&self) -> f32 {
        let minutes_after_sunrise = (Self::WORKDAY_START_CLOCK_MINUTE - Self::SUNRISE_CLOCK_MINUTE)
            .rem_euclid(Self::CLOCK_MINUTES_PER_DAY);
        self.cycle_seconds_for_clock_minutes(minutes_after_sunrise)
    }

    pub fn ordinary_work_end_seconds(&self) -> f32 {
        let minutes_after_sunrise = (Self::WORKDAY_END_CLOCK_MINUTE - Self::SUNRISE_CLOCK_MINUTE)
            .rem_euclid(Self::CLOCK_MINUTES_PER_DAY);
        self.cycle_seconds_for_clock_minutes(minutes_after_sunrise)
    }

    pub fn ordinary_shift_seconds(&self) -> f32 {
        (self.ordinary_work_end_seconds() - self.ordinary_work_start_seconds()).max(0.0)
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

    /// Linear display clock, normalized over 24 hours (0 = midnight).
    ///
    /// `seconds_in_cycle` starts at sunrise for the existing calendar and
    /// economy cadence, but this mapping advances uniformly through daylight,
    /// sunset, night and sunrise. At the default 1x durations, one simulation
    /// second is exactly one displayed world minute.
    pub fn normalized_time(&self) -> f32 {
        let cycle = self.cycle_duration();
        if cycle <= 0.0 {
            return 0.5;
        }
        (Self::SUNRISE_NORMALIZED + self.seconds_in_cycle / cycle).rem_euclid(1.0)
    }

    /// Sun position phase for lighting: elevation == `-cos(sun_phase())`,
    /// exactly the old `normalized * TAU` convention (PI/2 = sunrise on the
    /// horizon, PI = solar noon, 3*PI/2 = sunset). Derived from cycle
    /// fractions, so its variable visual speed is independent of the linear
    /// display clock.
    pub fn sun_phase(&self) -> f32 {
        use std::f32::consts::PI;
        if self.is_day() {
            PI * 0.5 + self.day_t() * PI
        } else {
            PI * 1.5 + self.night_t() * PI
        }
    }

    /// Set the linear display clock using a normalized time (0.0-1.0).
    ///
    /// Deliberately leaves `day` untouched: this is a debug reposition within the
    /// current day, not the passage of time.
    pub fn set_normalized_time(&mut self, normalized: f32) {
        let cycle = self.cycle_duration();
        if cycle <= 0.0 {
            return;
        }

        let cycle_pos = (normalized.rem_euclid(1.0) - Self::SUNRISE_NORMALIZED).rem_euclid(1.0);
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
    fn display_clock_is_linear_while_the_sun_uses_summer_hours() {
        let mut wt = WorldTime::new(
            WorldTime::DEFAULT_DAY_DURATION,
            WorldTime::DEFAULT_NIGHT_DURATION,
            0.0,
        );
        // The internal cycle still begins at sunrise.
        assert!((wt.normalized_time() - WorldTime::SUNRISE_NORMALIZED).abs() < 1e-4);
        // Daylight ends at 23:00.
        wt.seconds_in_cycle = wt.day_duration;
        assert!((wt.normalized_time() - WorldTime::SUNSET_NORMALIZED).abs() < 2e-3);
        // The sun's physical phase still reaches its zenith halfway through
        // its long daylight arc (14:00 with a 05:00 sunrise and 23:00 sunset).
        wt.seconds_in_cycle = wt.day_duration * 0.5;
        assert!((wt.sun_phase() - std::f32::consts::PI).abs() < 1e-3);
        assert!((-wt.sun_phase().cos() - 1.0).abs() < 1e-3);
        // Midnight round-trips through the inverse mapping.
        wt.set_normalized_time(0.0);
        assert!(!wt.is_day());
        assert!((wt.normalized_time() - 0.0).rem_euclid(1.0) < 2e-3);
    }

    #[test]
    fn one_simulation_second_is_one_displayed_minute_at_every_hour() {
        assert_eq!(
            WorldTime::new_default().cycle_duration(),
            WorldTime::CLOCK_MINUTES_PER_DAY
        );
        for hour in [5.0_f32, 12.0, 22.5, 23.5, 0.5] {
            let mut wt = WorldTime::new_default();
            wt.set_normalized_time(hour / 24.0);
            let before = wt.normalized_time();
            wt.advance(1.0, 0.0);
            let advanced_minutes =
                (wt.normalized_time() - before).rem_euclid(1.0) * WorldTime::CLOCK_MINUTES_PER_DAY;
            assert!((advanced_minutes - 1.0).abs() < 1e-3, "hour={hour}");
        }
    }

    #[test]
    fn only_the_below_horizon_sun_arc_accelerates() {
        let mut daylight = WorldTime::new_default();
        daylight.set_normalized_time(12.0 / 24.0);
        let daylight_phase = daylight.sun_phase();
        daylight.advance(1.0, 0.0);
        let daylight_step = daylight.sun_phase() - daylight_phase;

        let mut night = WorldTime::new_default();
        night.set_normalized_time(0.0);
        let night_phase = night.sun_phase();
        night.advance(1.0, 0.0);
        let night_step = night.sun_phase() - night_phase;

        assert!((night_step / daylight_step - 3.0).abs() < 1e-3);
        assert!((daylight.normalized_time() * 24.0 - (12.0 + 1.0 / 60.0)).abs() < 1e-4);
        assert!((night.normalized_time() * 24.0 - 1.0 / 60.0).abs() < 1e-4);
    }

    #[test]
    fn work_schedule_uses_clock_hours_not_sun_arc_fractions() {
        let mut wt = WorldTime::new_default();
        wt.set_normalized_time(5.99 / 24.0);
        assert!(!wt.is_ordinary_work_time());
        wt.set_normalized_time(6.0 / 24.0);
        assert!(wt.is_ordinary_work_time());
        wt.set_normalized_time(17.99 / 24.0);
        assert!(wt.is_ordinary_work_time());
        wt.set_normalized_time(18.0 / 24.0);
        assert!(!wt.is_ordinary_work_time());
        assert!((WorldTime::new_default().ordinary_shift_seconds() - 12.0 * 60.0).abs() < 1e-3);
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
