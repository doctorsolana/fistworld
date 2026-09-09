//! Raw frame intervals for connected rehearsals, kept in memory until summary.
//! They include frame pacing and capture costs, and are not CPU/GPU timings.

#[derive(Clone, Copy, Default)]
pub(super) enum Phase {
    #[default]
    Approach = 0,
    Combat = 1,
    CloseUp = 2,
}

#[derive(Default)]
pub(super) struct FrameTiming {
    started: bool,
    phase: Phase,
    frames: [Vec<f64>; 3],
    clock: ClockCadence,
    presentation_clock: ClockCadence,
    focused_frames: u64,
    unfocused_frames: u64,
    unknown_focus_frames: u64,
}

impl FrameTiming {
    /// Called before screenshot-ticket waits. Time<Real>'s current delta is the
    /// completed preceding frame, so attribute it to that frame's camera phase.
    pub(super) fn record(&mut self, active: bool, seconds: f64) -> bool {
        if !active {
            return false;
        }
        if !self.started {
            // The first active delta can include the readiness screenshot.
            self.started = true;
            return false;
        }
        if seconds.is_finite() && seconds > 0. {
            self.frames[self.phase as usize].push(seconds * 1000.);
            return true;
        }
        false
    }

    pub(super) fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    pub(super) fn record_focus(&mut self, focused: Option<bool>) {
        match focused {
            Some(true) => self.focused_frames += 1,
            Some(false) => self.unfocused_frames += 1,
            None => self.unknown_focus_frames += 1,
        }
    }

    pub(super) fn record_clock(
        &mut self,
        active: bool,
        running: bool,
        world_seconds: Option<f64>,
        presentation_seconds: Option<f64>,
        frame_seconds: f64,
    ) {
        if active {
            self.clock.record(running, world_seconds, frame_seconds);
            self.presentation_clock
                .record(running, presentation_seconds, frame_seconds);
        }
    }

    pub(super) fn summary(&self) -> serde_json::Value {
        let all: Vec<_> = self.frames.iter().flatten().copied().collect();
        serde_json::json!({
            "source":"Time<Real>",
            "includes_frame_pacing":true,
            "excludes_readiness_and_initial_screenshots":true,
            "all":statistics(&all),
            "approach":statistics(&self.frames[Phase::Approach as usize]),
            "combat":statistics(&self.frames[Phase::Combat as usize]),
            "close_up":statistics(&self.frames[Phase::CloseUp as usize]),
            "replicated_clock":self.clock.summary(),
            "presentation_clock":self.presentation_clock.summary(),
            "window_focus":{
                "focused_frames":self.focused_frames,"unfocused_frames":self.unfocused_frames,
                "unknown_frames":self.unknown_focus_frames,
            },
        })
    }
}

#[derive(Default)]
struct ClockCadence {
    previous: Option<f64>,
    comparisons: u64,
    unchanged_frames: u64,
    backwards_steps: u64,
    advancing_frames: u64,
    largest_step_seconds: f64,
    held_wall_seconds: f64,
    longest_hold_wall_seconds: f64,
}
impl ClockCadence {
    fn record(&mut self, running: bool, now: Option<f64>, frame_seconds: f64) {
        let Some(now) = now.filter(|time| running && time.is_finite()) else {
            self.previous = None;
            self.held_wall_seconds = 0.;
            return;
        };
        let Some(previous) = self.previous.replace(now) else {
            return;
        };
        self.comparisons += 1;
        let delta = now - previous;
        if delta == 0. {
            self.unchanged_frames += 1;
            if frame_seconds.is_finite() && frame_seconds > 0. {
                self.held_wall_seconds += frame_seconds;
            }
            self.longest_hold_wall_seconds =
                self.longest_hold_wall_seconds.max(self.held_wall_seconds);
        } else {
            self.held_wall_seconds = 0.;
            if delta > 0. {
                self.advancing_frames += 1;
                self.largest_step_seconds = self.largest_step_seconds.max(delta);
            } else {
                self.backwards_steps += 1;
            }
        }
    }
    fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "running_frame_comparisons":self.comparisons,"unchanged_frames":self.unchanged_frames,
            "advancing_frames":self.advancing_frames,"backwards_steps":self.backwards_steps,
            "largest_advance_world_seconds":self.largest_step_seconds,
            "longest_unchanged_wall_seconds":self.longest_hold_wall_seconds,
        })
    }
}

fn statistics(frames: &[f64]) -> serde_json::Value {
    if frames.is_empty() {
        return serde_json::json!({"samples":0,"elapsed_seconds":0.});
    }
    let mut sorted = frames.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    let percentile = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let total_ms: f64 = frames.iter().sum();
    serde_json::json!({
        "samples":frames.len(),"elapsed_seconds":total_ms / 1000.,
        "mean_fps":frames.len() as f64 * 1000. / total_ms,
        "mean_ms":total_ms / frames.len() as f64,
        "p50_ms":percentile(0.50),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99),
        "max_ms":sorted.last().unwrap(),
        "frames_over_35_ms":frames.iter().filter(|ms| **ms > 35.).count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_excludes_readiness_and_preserves_long_frames_in_their_previous_phase() {
        let mut timing = FrameTiming::default();
        timing.record(false, 4.);
        timing.record(true, 2.); // final readiness screenshot
        timing.record(true, 0.010);
        timing.set_phase(Phase::Combat);
        timing.record(true, 0.020);
        timing.record(true, 0.500); // no simulation-clock clamp
        timing.set_phase(Phase::CloseUp);
        timing.record(true, 0.040);
        let summary = timing.summary();
        assert_eq!(summary["all"]["samples"], 4);
        assert!((summary["all"]["elapsed_seconds"].as_f64().unwrap() - 0.57).abs() < 1e-9);
        assert_eq!(summary["approach"]["samples"], 1);
        assert_eq!(summary["combat"]["samples"], 2);
        assert_eq!(summary["combat"]["p99_ms"], 500.);
        assert_eq!(summary["all"]["frames_over_35_ms"], 2);
        assert_eq!(summary["close_up"]["samples"], 1);
    }

    #[test]
    fn replicated_clock_counts_stale_frames_without_treating_pause_as_a_stall() {
        let mut clock = ClockCadence::default();
        for now in [10., 10., 10., 10.1] {
            clock.record(true, Some(now), 0.02);
        }
        clock.record(false, Some(10.1), 5.);
        clock.record(true, Some(10.1), 0.02);
        assert_eq!(clock.comparisons, 3);
        assert_eq!(clock.unchanged_frames, 2);
        assert_eq!(clock.advancing_frames, 1);
        assert!((clock.longest_hold_wall_seconds - 0.04).abs() < 1e-9);
    }

    #[test]
    fn clocks_and_window_focus_are_separate_from_frame_costs() {
        let mut timing = FrameTiming::default();
        assert!(!timing.record(true, 1.));
        timing.record_clock(true, true, Some(10.), Some(10.), 0.02);
        assert!(timing.record(true, 0.02));
        timing.record_focus(Some(false));
        timing.record_clock(true, true, Some(10.), Some(10.02), 0.02);
        let summary = timing.summary();
        assert_eq!(summary["all"]["samples"], 1);
        assert_eq!(summary["window_focus"]["unfocused_frames"], 1);
        assert_eq!(summary["replicated_clock"]["unchanged_frames"], 1);
        assert_eq!(summary["presentation_clock"]["advancing_frames"], 1);
    }
}
