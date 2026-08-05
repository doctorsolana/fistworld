//! The single source of truth for real and accelerated simulation deltas.
//!
//! Systems should never multiply `Time` by `TimeWarp` themselves. Reading this
//! parameter makes the choice explicit: cooldowns and CPU budgets use real
//! seconds; movement, work and world decisions use world seconds.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::TimeWarp;

#[derive(SystemParam)]
pub struct SimulationTime<'w, 's> {
    time: Option<Res<'w, Time>>,
    warp: Query<'w, 's, &'static TimeWarp>,
}

impl SimulationTime<'_, '_> {
    pub fn real_seconds(&self) -> f32 {
        self.time
            .as_ref()
            .map_or(1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32, |time| {
                time.delta_secs()
            })
    }

    pub fn real_seconds_f64(&self) -> f64 {
        f64::from(self.real_seconds())
    }

    pub fn world_seconds(&self) -> f32 {
        self.real_seconds() * self.factor()
    }

    pub fn elapsed_real_seconds_f64(&self) -> f64 {
        self.time
            .as_ref()
            .map_or(0.0, |time| time.elapsed_secs_f64())
    }

    pub fn factor(&self) -> f32 {
        self.warp.iter().next().map_or(1.0, |warp| warp.0)
    }
}
