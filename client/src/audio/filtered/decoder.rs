use super::*;
use bevy::audio::Source;
use std::time::Duration;

/// Two stable nonresonant poles. Coefficients interpolate over 20 ms after a
/// control change; the target is polled every 128 samples, never per sample.
pub(crate) struct FilteredDecoder {
    clip: Arc<MonoClip>,
    control: FilterControl,
    looping: bool,
    cursor: usize,
    first: f32,
    second: f32,
    alpha: f32,
    target_alpha: f32,
    alpha_step: f32,
    ramp_left: usize,
    poll_left: usize,
    last_cutoff: f32,
}

fn coefficient(cutoff: f32, rate: SampleRate) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff.min(rate.get() as f32 * 0.45) / rate.get() as f32).exp()
}

impl FilteredDecoder {
    pub(super) fn new(clip: Arc<MonoClip>, control: FilterControl, looping: bool) -> Self {
        let last_cutoff = control.cutoff_hz();
        let alpha = coefficient(last_cutoff, clip.rate);
        Self {
            clip,
            control,
            looping,
            cursor: 0,
            first: 0.0,
            second: 0.0,
            alpha,
            target_alpha: alpha,
            alpha_step: 0.0,
            ramp_left: 0,
            poll_left: 0,
            last_cutoff,
        }
    }
}

impl Iterator for FilteredDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.cursor == self.clip.samples.len() {
            if !self.looping {
                return None;
            }
            self.cursor = 0;
        }
        if self.poll_left == 0 {
            let cutoff = self.control.cutoff_hz();
            if cutoff != self.last_cutoff {
                self.last_cutoff = cutoff;
                self.target_alpha = coefficient(cutoff, self.clip.rate);
                self.ramp_left = (self.clip.rate.get() / 50).max(1) as usize;
                self.alpha_step = (self.target_alpha - self.alpha) / self.ramp_left as f32;
            }
            self.poll_left = 128;
        }
        self.poll_left -= 1;
        if self.ramp_left > 0 {
            self.alpha += self.alpha_step;
            self.ramp_left -= 1;
            if self.ramp_left == 0 {
                self.alpha = self.target_alpha;
            }
        }
        let sample = self.clip.samples[self.cursor];
        self.cursor += 1;
        self.first += self.alpha * (sample - self.first);
        self.second += self.alpha * (self.first - self.second);
        Some(self.second)
    }
}

impl Source for FilteredDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        self.clip.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        (!self.looping).then(|| {
            Duration::from_secs_f64(self.clip.samples.len() as f64 / self.clip.rate.get() as f64)
        })
    }
}
