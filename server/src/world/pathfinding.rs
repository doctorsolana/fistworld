//! Shared server budget for bounded tactical route planning.
//!
//! Village-road routing owns the active pathfinding implementation. Keeping
//! its CPU budget in this small module lets runtime, labs, and tests configure
//! the same limits without retaining the superseded per-agent grid A*.

use bevy::prelude::*;
use std::time::Duration;

// A hard request cap protects a tick even when every lookup is a cheap cache
// hit. The wall-clock budget is normally the first limit reached.
const DEFAULT_PATHFINDING_REQUESTS_PER_TICK: usize = 32;
const DEFAULT_PATHFINDING_MILLISECONDS_PER_TICK: f32 = 4.0;

#[derive(Resource, Clone, Debug)]
pub struct PathfindingBudgetSettings {
    pub max_requests_per_tick: usize,
    pub max_milliseconds_per_tick: f32,
}

impl Default for PathfindingBudgetSettings {
    fn default() -> Self {
        let max_requests_per_tick = std::env::var("CITYSIM_PATHFINDING_REQUESTS_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_PATHFINDING_REQUESTS_PER_TICK)
            .clamp(1, 128);
        let max_milliseconds_per_tick = std::env::var("CITYSIM_PATHFINDING_MILLISECONDS_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|milliseconds| milliseconds.is_finite() && *milliseconds > 0.0)
            .unwrap_or(DEFAULT_PATHFINDING_MILLISECONDS_PER_TICK)
            .clamp(0.1, 50.0);
        Self {
            max_requests_per_tick,
            max_milliseconds_per_tick,
        }
    }
}

impl PathfindingBudgetSettings {
    /// Spend bounded server headroom when real simulation work has either
    /// accumulated or waited long enough to become visible to a player.
    ///
    /// Count alone is insufficient: three difficult porter routes can be far
    /// more important than a hundred optional strolls. Wait age therefore
    /// opens the emergency budget even for a small essential queue. Cosmetic
    /// ambient routes are deliberately not included by the caller.
    pub fn max_duration_for_pressure(
        &self,
        committed_pending: usize,
        oldest_committed_wait_seconds: f64,
    ) -> Duration {
        let target_milliseconds = if committed_pending >= 64 {
            6.0
        } else if committed_pending >= 16 {
            5.0
        } else if oldest_committed_wait_seconds >= 2.0 {
            // A small pathological queue must not reserve most of the server
            // tick forever. Its guaranteed committed-first slot already
            // provides progress; this modest age boost improves latency while
            // preserving 10x simulation throughput.
            4.5
        } else {
            self.max_milliseconds_per_tick
        };
        // An explicit operator setting above the adaptive ceiling remains
        // authoritative. Lower settings retain adaptive recovery so a brief
        // burst cannot strand embodied work indefinitely.
        let milliseconds = target_milliseconds.max(self.max_milliseconds_per_tick);
        Duration::from_secs_f32(milliseconds / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_pressure_can_use_bounded_server_headroom() {
        let settings = PathfindingBudgetSettings {
            max_requests_per_tick: 32,
            max_milliseconds_per_tick: 4.0,
        };
        assert_eq!(settings.max_duration_for_pressure(15, 0.74).as_millis(), 4);
        assert_eq!(settings.max_duration_for_pressure(16, 0.0).as_millis(), 5);
        assert_eq!(settings.max_duration_for_pressure(64, 0.0).as_millis(), 6);
        assert_eq!(
            settings.max_duration_for_pressure(3, 2.0).as_micros(),
            4_500,
            "a few visibly starved freight routes need recovery headroom too",
        );

        let operator_override = PathfindingBudgetSettings {
            max_requests_per_tick: 32,
            max_milliseconds_per_tick: 10.0,
        };
        assert_eq!(
            operator_override
                .max_duration_for_pressure(1_000, 30.0)
                .as_millis(),
            10
        );
    }
}
