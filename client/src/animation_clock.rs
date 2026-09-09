//! Smooth cosmetic action time between authoritative clock packets. This never
//! writes WorldTime or makes a simulation decision. Offline captures pin it.
use bevy::prelude::*;
use shared::components::{TimeWarp, WorldTime};

/// At the normal 33 ms send interval this covers three missing snapshots, then
/// holds. Warped playback gets the same real-time outage budget.
const MAX_AHEAD_REAL_SECONDS: f64 = 0.10;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AnimationClockUpdate;

#[derive(Clone, Copy)]
struct Source {
    entity: Entity,
    seconds: f64,
    cycle: f64,
    factor: f64,
    received_at: f64,
}

#[derive(Resource, Default)]
pub(crate) struct AnimationClock {
    source: Option<Source>,
    presented: f64,
    updated_at: f64,
    exact: bool,
}

#[derive(serde::Serialize, Debug, Clone, Copy)]
pub(crate) struct AnimationClockInspection {
    pub source_seconds: f64,
    pub presented_seconds: f64,
    pub source_age_seconds: f64,
    pub factor: f64,
    pub exact: bool,
}

pub(crate) fn seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

impl AnimationClock {
    /// Fixed capture fixtures own their timeline and must never receive an
    /// extra wall-clock advance on top of their explicit per-shot sampling.
    pub(crate) fn exact() -> Self {
        Self {
            exact: true,
            ..default()
        }
    }

    pub(crate) fn sample(&self, authoritative: &WorldTime) -> f64 {
        if self.exact || self.source.is_none() {
            seconds(authoritative)
        } else {
            self.presented
        }
    }

    pub(crate) fn inspection(&self) -> Option<AnimationClockInspection> {
        let source = self.source?;
        Some(AnimationClockInspection {
            source_seconds: source.seconds,
            presented_seconds: self.presented,
            source_age_seconds: (self.updated_at - source.received_at).max(0.),
            factor: source.factor,
            exact: self.exact,
        })
    }

    fn advance(&mut self, incoming: Source, real_now: f64, received: bool) {
        let Some(previous) = self.source else {
            self.reset(incoming, real_now);
            return;
        };
        let age = (real_now - previous.received_at).max(0.);
        let elapsed = (real_now - self.updated_at).max(0.);
        let delta = incoming.seconds - previous.seconds;
        let jump_tolerance = (previous.factor.max(incoming.factor) * 0.4).max(1.);
        let jump =
            received && (delta < -jump_tolerance || delta - age * previous.factor > jump_tolerance);
        if self.exact
            || previous.entity != incoming.entity
            || previous.cycle != incoming.cycle
            || previous.factor != incoming.factor
            || incoming.factor == 0.
            || jump
            || real_now < self.updated_at
        {
            self.reset(incoming, real_now);
            return;
        }
        // A slightly older snapshot cannot pull a gait backwards or extend its
        // outage horizon. Explicit large clock rewinds are handled above.
        let latest = if received && incoming.seconds >= previous.seconds {
            incoming
        } else {
            previous
        };
        self.presented = (self.presented + elapsed * latest.factor)
            .max(latest.seconds)
            .min(latest.seconds + MAX_AHEAD_REAL_SECONDS * latest.factor);
        self.source = Some(latest);
        self.updated_at = real_now;
    }

    fn reset(&mut self, source: Source, real_now: f64) {
        self.presented = source.seconds;
        self.source = Some(source);
        self.updated_at = real_now;
    }
}

pub(crate) fn update(
    time: Res<Time<Real>>,
    clocks: Query<(Entity, Ref<WorldTime>, Option<&TimeWarp>)>,
    mut presentation: ResMut<AnimationClock>,
) {
    let Some((entity, clock, warp)) = clocks.iter().next() else {
        presentation.source = None;
        return;
    };
    let real_now = time.elapsed_secs_f64();
    presentation.advance(
        Source {
            entity,
            seconds: seconds(&clock),
            cycle: f64::from(clock.cycle_duration()),
            factor: f64::from(TimeWarp::clamped(warp.map_or(1., |w| w.0)).0),
            received_at: real_now,
        },
        real_now,
        clock.is_changed(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(seconds: f64, factor: f64, at: f64) -> Source {
        Source {
            entity: Entity::from_bits(1),
            seconds,
            cycle: 1440.,
            factor,
            received_at: at,
        }
    }
    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }

    #[test]
    fn advances_each_render_frame_between_packets_and_never_rewinds_on_normal_arrivals() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(10., 1., 0.), 0., true);
        for frame in 1..=3 {
            clock.advance(sample(10., 1., 0.), frame as f64 / 120., false);
            near(clock.presented, 10. + frame as f64 / 120.);
        }
        // A late 20 ms server sample arrives after 33 ms locally.
        clock.advance(sample(10.020, 1., 1. / 30.), 1. / 30., true);
        near(clock.presented, 10. + 1. / 30.);
        // Reordered old data cannot move us backwards either.
        clock.advance(sample(10.010, 1., 0.04), 0.04, true);
        near(clock.presented, 10.04);
    }

    #[test]
    fn stale_clock_stops_at_the_bounded_horizon_and_recovers_on_a_new_packet() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(10., 1., 0.), 0., true);
        clock.advance(sample(10., 1., 0.), 0.8, false);
        near(clock.presented, 10.1);
        clock.advance(sample(10., 1., 0.), 1.2, false);
        near(clock.presented, 10.1);
        clock.advance(sample(11.2, 1., 1.2), 1.2, true);
        near(clock.presented, 11.2);
    }

    #[test]
    fn pause_snaps_to_authority_and_unpause_restores_the_requested_rate() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(10., 1., 0.), 0., true);
        clock.advance(sample(10., 1., 0.), 0.05, false);
        clock.advance(sample(10.02, 0., 0.06), 0.06, true);
        near(clock.presented, 10.02);
        clock.advance(sample(10.02, 0., 0.06), 5., false);
        near(clock.presented, 10.02);
        clock.advance(sample(10.02, 10., 5.), 5., true);
        clock.advance(sample(10.02, 10., 5.), 5.05, false);
        near(clock.presented, 10.52);
        clock.advance(sample(10.02, 10., 5.), 7., false);
        near(clock.presented, 11.02);
    }

    #[test]
    fn slow_motion_uses_its_real_factor() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(10., 0.1, 0.), 0., true);
        clock.advance(sample(10., 0.1, 0.), 0.05, false);
        near(clock.presented, 10.005);
    }

    #[test]
    fn large_debug_jumps_and_new_world_identities_reset_the_timeline() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(10., 1., 0.), 0., true);
        clock.advance(sample(100., 1., 0.03), 0.03, true);
        near(clock.presented, 100.);
        clock.advance(sample(20., 1., 0.06), 0.06, true);
        near(clock.presented, 20.);
        let new = Source {
            entity: Entity::from_bits(2),
            ..sample(20.01, 1., 0.07)
        };
        clock.advance(new, 0.07, true);
        near(clock.presented, 20.01);
    }

    #[test]
    fn day_rollover_is_continuous_and_cycle_changes_resync() {
        let mut clock = AnimationClock::default();
        clock.advance(sample(1439.98, 1., 0.), 0., true);
        clock.advance(sample(1440.02, 1., 0.04), 0.04, true);
        near(clock.presented, 1440.02);
        let new = Source {
            cycle: 60.,
            ..sample(60.02, 1., 0.05)
        };
        clock.advance(new, 0.05, true);
        near(clock.presented, 60.02);
    }

    #[test]
    fn offline_exact_mode_never_double_advances_an_authored_timeline() {
        let mut clock = AnimationClock::exact();
        clock.advance(sample(10., 1., 0.), 0., true);
        clock.advance(sample(10., 1., 0.), 8., false);
        near(clock.presented, 10.);
        clock.advance(sample(9.99, 1., 8.01), 8.01, true);
        near(clock.presented, 9.99);
        assert!(clock.inspection().unwrap().exact);
    }

    #[test]
    fn exact_sampling_observes_fixture_changes_later_in_the_same_frame() {
        let mut clock = AnimationClock::exact();
        clock.advance(sample(10., 1., 0.), 0., true);
        // Capture setup can author the next shot after the presentation update.
        // Sampling must use that fixture time, not the cached previous shot.
        let fixture = WorldTime::new(1080., 360., 20.);
        near(clock.sample(&fixture), 20.);
        near(AnimationClock::default().sample(&fixture), 20.);
    }
}
