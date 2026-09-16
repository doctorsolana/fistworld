//! One clock-hour contract for day plans, tactical jobs and aggregate work.

use shared::components::WorldTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkHours {
    start_minute: u16,
    end_minute: u16,
}

pub(crate) const ORDINARY: WorkHours = WorkHours {
    start_minute: WorldTime::WORKDAY_START_CLOCK_MINUTE as u16,
    end_minute: WorldTime::WORKDAY_END_CLOCK_MINUTE as u16,
};

pub(crate) const TAVERN: WorkHours = WorkHours {
    start_minute: 12 * 60,
    end_minute: 21 * 60 + 30,
};

impl WorkHours {
    pub(crate) const fn clock_minutes(self) -> (u16, u16) {
        (self.start_minute, self.end_minute)
    }

    pub(crate) fn contains(self, clock: &WorldTime) -> bool {
        let cycle = f64::from(clock.cycle_duration());
        if cycle <= 0.0 {
            return false;
        }
        let (start, duration) = self.cycle_interval(clock);
        (f64::from(clock.seconds_in_cycle) - start).rem_euclid(cycle) < duration
    }

    fn cycle_interval(self, clock: &WorldTime) -> (f64, f64) {
        let minutes_per_day = f64::from(WorldTime::CLOCK_MINUTES_PER_DAY);
        let seconds_per_minute = f64::from(clock.cycle_duration()) / minutes_per_day;
        let start = (f64::from(self.start_minute) - f64::from(WorldTime::SUNRISE_CLOCK_MINUTE))
            .rem_euclid(minutes_per_day)
            * seconds_per_minute;
        let duration = (f64::from(self.end_minute) - f64::from(self.start_minute))
            .rem_euclid(minutes_per_day)
            * seconds_per_minute;
        (start, duration)
    }

    /// Exact overlap, including shifts across midnight and several elapsed
    /// days. Interval integration uses the same hours as each worker without
    /// adding per-person tactical ticks or granting work during the night.
    pub(crate) fn productive_seconds_ending_at(self, clock: &WorldTime, elapsed: f64) -> f64 {
        let cycle = f64::from(clock.cycle_duration());
        if cycle <= 0.0 || elapsed <= 0.0 {
            return 0.0;
        }
        let (work_start, duration) = self.cycle_interval(clock);
        let end = f64::from(clock.day) * cycle + f64::from(clock.seconds_in_cycle);
        let start = (end - elapsed).max(0.0);
        let cumulative = |seconds: f64| {
            let relative = seconds - work_start;
            (relative / cycle).floor() * duration + relative.rem_euclid(cycle).min(duration)
        };
        (cumulative(end) - cumulative(start)).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregated_work_matches_observed_work_across_shifts_and_short_clocks() {
        for cycle in [1440.0, 240.0] {
            for hours in [
                ORDINARY,
                TAVERN,
                WorkHours {
                    start_minute: 22 * 60,
                    end_minute: 4 * 60,
                },
            ] {
                let mut clock = WorldTime::new(cycle * 0.75, cycle * 0.25, 0.0);
                // Midpoint samples avoid counting the same shift boundary
                // twice. Three days also cover the midnight service case.
                let mut physical = 0.0;
                for tick in 0..(cycle as u32 * 12) {
                    let absolute = (f64::from(tick) + 0.5) / 4.0;
                    clock.day = (absolute / f64::from(cycle)) as u32;
                    clock.seconds_in_cycle = absolute.rem_euclid(f64::from(cycle)) as f32;
                    if hours.contains(&clock) {
                        physical += 0.25;
                    }
                }
                clock.day = 3;
                clock.seconds_in_cycle = 0.0;
                let aggregate = hours.productive_seconds_ending_at(&clock, f64::from(cycle) * 3.0);
                assert!(
                    (aggregate - physical).abs() < 0.001,
                    "{hours:?}: {aggregate} / {physical}"
                );
            }
        }
    }
}
