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
    /// 20 minutes of daylight.
    pub const DEFAULT_DAY_DURATION: f32 = 20.0 * 60.0;
    /// 7 minutes of night (shorter nights).
    pub const DEFAULT_NIGHT_DURATION: f32 = 7.0 * 60.0;
    /// Start early morning (near sunrise).
    pub const DEFAULT_START_SECONDS_IN_DAY: f32 = 30.0;

    pub fn new(day_duration: f32, night_duration: f32, seconds_in_cycle: f32) -> Self {
        let mut wt = Self {
            seconds_in_cycle,
            day_duration,
            night_duration,
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

    pub fn advance(&mut self, dt: f32) {
        self.seconds_in_cycle += dt.max(0.0);
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
