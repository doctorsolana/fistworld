//! The single source of truth for real and accelerated simulation deltas.
//!
//! Systems should never multiply `Time` by `TimeWarp` themselves. Reading this
//! parameter makes the choice explicit: cooldowns and CPU budgets use real
//! seconds; movement, work and world decisions use world seconds.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::TimeWarp;

/// One captured fixed-tick delta shared by every simulation system.
///
/// Capturing once prevents a newly added mechanic from inventing its own warp
/// multiplication or observing a different factor halfway through a tick.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SimulationDelta {
    real_seconds: f32,
    world_seconds: f32,
    factor: f32,
    elapsed_real_seconds: f64,
}

impl Default for SimulationDelta {
    fn default() -> Self {
        let real_seconds = 1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32;
        Self {
            real_seconds,
            world_seconds: real_seconds,
            factor: 1.0,
            elapsed_real_seconds: 0.0,
        }
    }
}

/// Capture the master speed once, at the start of the shared simulation
/// schedule. This is the only production system that multiplies by TimeWarp.
pub fn refresh_simulation_delta(
    time: Option<Res<Time>>,
    warp: Query<&TimeWarp>,
    mut delta: ResMut<SimulationDelta>,
) {
    let real_seconds = time
        .as_ref()
        .map_or(1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32, |time| {
            time.delta_secs()
        });
    let factor = warp.iter().next().map_or(1.0, |warp| warp.0);
    *delta = SimulationDelta {
        real_seconds,
        world_seconds: real_seconds * factor,
        factor,
        elapsed_real_seconds: time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64()),
    };
}

#[derive(SystemParam)]
pub struct SimulationTime<'w, 's> {
    captured: Option<Res<'w, SimulationDelta>>,
    // Standalone unit tests may install a simulation system without the shared
    // schedule. These two inputs preserve that small-test ergonomics while the
    // live server and Village Lab always consume the captured resource above.
    time: Option<Res<'w, Time>>,
    warp: Query<'w, 's, &'static TimeWarp>,
}

impl SimulationTime<'_, '_> {
    pub fn real_seconds(&self) -> f32 {
        self.captured.as_ref().map_or_else(
            || {
                self.time
                    .as_ref()
                    .map_or(1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32, |time| {
                        time.delta_secs()
                    })
            },
            |delta| delta.real_seconds,
        )
    }

    pub fn real_seconds_f64(&self) -> f64 {
        f64::from(self.real_seconds())
    }

    pub fn world_seconds(&self) -> f32 {
        self.captured.as_ref().map_or_else(
            || self.real_seconds() * self.factor(),
            |delta| delta.world_seconds,
        )
    }

    pub fn elapsed_real_seconds_f64(&self) -> f64 {
        self.captured.as_ref().map_or_else(
            || {
                self.time
                    .as_ref()
                    .map_or(0.0, |time| time.elapsed_secs_f64())
            },
            |delta| delta.elapsed_real_seconds,
        )
    }

    pub fn factor(&self) -> f32 {
        self.captured.as_ref().map_or_else(
            || self.warp.iter().next().map_or(1.0, |warp| warp.0),
            |delta| delta.factor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Observed(Vec<(f32, f32)>);

    fn observe(delta: SimulationTime, mut observed: ResMut<Observed>) {
        observed
            .0
            .push((delta.real_seconds(), delta.world_seconds()));
    }

    #[test]
    fn one_captured_delta_drives_every_warp_factor() {
        let mut app = App::new();
        app.init_resource::<SimulationDelta>()
            .init_resource::<Observed>()
            .add_systems(Update, (refresh_simulation_delta, observe).chain());
        let clock = app.world_mut().spawn(TimeWarp::clamped(25.0)).id();
        app.update();
        app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 100.0;
        app.update();

        let values = &app.world().resource::<Observed>().0;
        assert!((values[0].1 / values[0].0 - 25.0).abs() < 0.01);
        assert!((values[1].1 / values[1].0 - 100.0).abs() < 0.01);
    }
}
