//! Shared server budget for bounded tactical route planning.
//!
//! Village-road routing owns the active pathfinding implementation. Keeping
//! its CPU budget in this small module lets runtime, labs, and tests configure
//! the same limits without retaining the superseded per-agent grid A*.

use bevy::prelude::*;
use std::time::Duration;

// A hard request cap protects a tick even when every lookup is a cheap cache
// hit. The wall-clock budget is normally the first limit reached.
const DEFAULT_PATHFINDING_REQUESTS_PER_TICK: usize = 16;
const DEFAULT_PATHFINDING_MILLISECONDS_PER_TICK: f32 = 2.0;

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
    pub fn max_duration(&self) -> Duration {
        Duration::from_secs_f32(self.max_milliseconds_per_tick / 1_000.0)
    }
}
