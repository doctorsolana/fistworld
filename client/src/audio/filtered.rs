//! Shared short-clip PCM and a per-voice low-pass filter through Bevy's native
//! Decodable extension. Nothing is regenerated on distance/zoom changes.
//!
//! The loop lives inside the decoder. Bevy/Rodio's outer repeat_infinite buffers
//! the first pass, which would freeze a filter wrapped inside that pass.

mod cache;
mod decoder;
#[cfg(test)]
mod tests;

use bevy::{
    audio::{AddAudioSource, ChannelCount, SampleRate},
    prelude::*,
};
pub(crate) use cache::WorldSoundCache;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

pub(super) fn install(app: &mut App) {
    app.add_audio_source::<FilteredSound>()
        .init_resource::<WorldSoundCache>()
        .add_systems(
            PostUpdate,
            prevent_outer_loop
                .after(super::sfx::SfxSet::Playback)
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

fn prevent_outer_loop(mut voices: Query<&mut PlaybackSettings, With<AudioPlayer<FilteredSound>>>) {
    for mut settings in &mut voices {
        if matches!(settings.mode, bevy::audio::PlaybackMode::Loop) {
            // Outer buffering would retain an infinite stream or freeze a finite
            // filtered pass. Loop ownership is always the source's constructor.
            settings.mode = bevy::audio::PlaybackMode::Once;
            warn!("Filtered sounds loop internally; corrected outer PlaybackMode::Loop");
        }
    }
}

/// The control thread only publishes a scalar; the audio thread owns all filter
/// history. No locks, allocation, ECS queries or asset access in the sample loop.
#[derive(Clone)]
pub(super) struct FilterControl(Arc<AtomicU32>);

impl FilterControl {
    pub fn new(cutoff_hz: f32) -> Self {
        let control = Self(Arc::new(AtomicU32::new(7000.0_f32.to_bits())));
        control.set_cutoff(cutoff_hz);
        control
    }

    pub fn set_cutoff(&self, cutoff_hz: f32) {
        if cutoff_hz.is_finite() {
            self.0.store(
                cutoff_hz.clamp(100.0, 20_000.0).to_bits(),
                Ordering::Relaxed,
            );
        }
    }

    pub fn cutoff_hz(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

pub(super) struct MonoClip {
    samples: Arc<[f32]>,
    rate: SampleRate,
}

impl MonoClip {
    pub fn bytes(&self) -> usize {
        self.samples.len() * size_of::<f32>()
    }
}

/// One cheap asset per admitted voice; clip samples are shared across voices.
/// Use PlaybackSettings::ONCE even for looping clips: the decoder owns looping.
#[derive(Asset, TypePath)]
pub(crate) struct FilteredSound {
    clip: Arc<MonoClip>,
    control: FilterControl,
    looping: bool,
}

impl FilteredSound {
    pub(super) fn new(clip: Arc<MonoClip>, looping: bool, cutoff_hz: f32) -> (Self, FilterControl) {
        let control = FilterControl::new(cutoff_hz);
        (
            Self {
                clip,
                control: control.clone(),
                looping,
            },
            control,
        )
    }
}

impl Decodable for FilteredSound {
    type Decoder = decoder::FilteredDecoder;
    fn decoder(&self) -> Self::Decoder {
        decoder::FilteredDecoder::new(self.clip.clone(), self.control.clone(), self.looping)
    }
}
