//! One non-spatial music voice, retained across scenery changes and pause menus.
//! Completed gameplay cues leave quiet space before repeating. The opening voyage
//! has priority; its completion is read from the actual audio sink, not a timer.

use super::{paths, AudioSettings};
use bevy::{asset::LoadState, audio::Volume, prelude::*};

#[cfg(test)]
mod tests;

const INITIAL_GAP_SECONDS: f32 = 4.0;
const REPEAT_GAP_SECONDS: f32 = 30.0;
const GAIN_RAMP_SECONDS: f32 = 0.08;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MusicGainRamp)]
pub(crate) enum MusicCue {
    Opening,
    Background,
}

/// One finite transition per changed preference. Settled playback is left alone,
/// including explicit playback overrides in the real-sink capture rehearsal.
#[derive(Component)]
pub(super) struct MusicGainRamp {
    current: f32,
    target: f32,
    remaining: f32,
}

impl Default for MusicGainRamp {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl MusicGainRamp {
    fn new(gain: f32) -> Self {
        Self {
            current: gain,
            target: gain,
            remaining: 0.0,
        }
    }

    fn retarget(&mut self, target: f32) {
        if target != self.target {
            self.target = target;
            self.remaining = GAIN_RAMP_SECONDS;
        }
    }

    fn advance(&mut self, dt: f32) -> Option<f32> {
        if self.remaining == 0.0 || dt <= 0.0 {
            return None;
        }
        let elapsed = dt.min(self.remaining);
        self.current += (self.target - self.current) * elapsed / self.remaining;
        self.remaining -= elapsed;
        if self.remaining == 0.0 {
            self.current = self.target;
        }
        Some(self.current)
    }
}

#[derive(Resource)]
pub(crate) struct MusicPlayback {
    opening_requested: bool,
    quiet_seconds: f32,
    background_failed: bool,
    background: Option<Handle<AudioSource>>,
}

impl Default for MusicPlayback {
    fn default() -> Self {
        Self {
            opening_requested: false,
            quiet_seconds: INITIAL_GAP_SECONDS,
            background_failed: false,
            background: None,
        }
    }
}

impl MusicPlayback {
    pub(crate) fn request_opening(&mut self) {
        self.opening_requested = true;
    }
}

pub(super) fn update_music(
    mut commands: Commands,
    settings: Res<AudioSettings>,
    time: Res<Time<Real>>,
    assets: Res<AssetServer>,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mut state: ResMut<MusicPlayback>,
    mut players: Query<(
        Entity,
        &MusicCue,
        &AudioPlayer,
        &mut PlaybackSettings,
        &mut MusicGainRamp,
        Option<&mut AudioSink>,
    )>,
) {
    let gain = settings.music_gain();
    if state.opening_requested {
        state.opening_requested = false;
        state.quiet_seconds = INITIAL_GAP_SECONDS;
        for (entity, _, _, _, _, sink) in &mut players {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
        }
        if gain > 0.0 {
            commands.spawn((
                Name::new("Opening voyage music"),
                MusicCue::Opening,
                MusicGainRamp::new(0.82 * gain),
                AudioPlayer::new(assets.load(paths::GAME_INTRO)),
                PlaybackSettings::ONCE.with_volume(Volume::Linear(0.82 * gain)),
            ));
        }
        return;
    }

    let cinematic_active = opening.is_some_and(|opening| opening.is_active());
    let mut occupied = false;
    for (entity, cue, player, mut initial, mut ramp, sink) in &mut players {
        occupied = true;
        if matches!(assets.get_load_state(&player.0), Some(LoadState::Failed(_))) {
            warn!("Music asset failed to load: {cue:?}");
            if *cue == MusicCue::Background {
                state.background_failed = true;
            }
            commands.entity(entity).despawn();
            continue;
        }
        if sink.as_ref().is_some_and(|sink| sink.empty()) {
            commands.entity(entity).despawn();
            state.quiet_seconds = if *cue == MusicCue::Background {
                REPEAT_GAP_SECONDS
            } else {
                INITIAL_GAP_SECONDS
            };
            continue;
        }
        // A disabled opening cue must not resume late over ordinary gameplay.
        if *cue == MusicCue::Opening && gain <= 0.0 {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            state.quiet_seconds = INITIAL_GAP_SECONDS;
            continue;
        }
        let paused = gain <= 0.0 || (*cue == MusicCue::Background && cinematic_active);
        let intended_gain = gain * if *cue == MusicCue::Opening { 0.82 } else { 1.0 };
        let volume = Volume::Linear(intended_gain);
        // Cover both a running sink and an asset still loading when the toggle changes.
        if initial.paused != paused {
            initial.paused = paused;
        }
        if initial.volume != volume {
            initial.volume = volume;
        }
        if settings.is_changed() {
            ramp.retarget(intended_gain);
        }
        if let Some(mut sink) = sink {
            if gain <= 0.0 {
                // Mute is immediate; resume ramps the retained background from
                // zero rather than restoring its former gain in one sample.
                if ramp.current != 0.0 {
                    sink.set_volume(Volume::SILENT);
                }
                *ramp = MusicGainRamp::new(0.0);
            } else if let Some(gain) = ramp.advance(time.delta_secs()) {
                sink.set_volume(Volume::Linear(gain));
            }
            if paused && !sink.is_paused() {
                sink.pause();
            }
            if !paused && sink.is_paused() {
                sink.play();
            }
        } else {
            // A sink created later uses the current initial volume directly.
            // It must not replay an old slider transition when loading finishes.
            *ramp = MusicGainRamp::new(intended_gain);
        }
    }
    if occupied || gain <= 0.0 || cinematic_active || state.background_failed {
        return;
    }
    state.quiet_seconds = (state.quiet_seconds - time.delta_secs()).max(0.0);
    if state.quiet_seconds > 0.0 {
        return;
    }
    let source = state
        .background
        .get_or_insert_with(|| assets.load(paths::BACKGROUND_MUSIC))
        .clone();
    commands.spawn((
        Name::new("Background music: The Chronicler's Quill"),
        MusicCue::Background,
        MusicGainRamp::new(gain),
        AudioPlayer::new(source),
        PlaybackSettings::ONCE.with_volume(Volume::Linear(gain)),
    ));
}

pub(super) fn stop_music(
    mut commands: Commands,
    players: Query<(Entity, Option<&AudioSink>), With<MusicCue>>,
    mut state: ResMut<MusicPlayback>,
) {
    for (entity, sink) in &players {
        if let Some(sink) = sink {
            sink.stop();
        }
        commands.entity(entity).despawn();
    }
    *state = MusicPlayback::default();
}
