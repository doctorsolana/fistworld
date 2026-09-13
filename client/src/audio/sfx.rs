//! UI actions expire in their input frame. Coalescing and admission happen before
//! AudioPlayer creation, so a click storm cannot allocate an unbounded decoder queue.

pub(crate) use super::catalog::{SfxAssets, SfxCue};
use super::AudioSettings;
use bevy::{audio::Volume, prelude::*};

pub(crate) const MAX_UI_VOICES: usize = 4;
const START_INTERVAL: f64 = 0.065;
const PENDING_TIMEOUT: f64 = 0.25;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SfxSet {
    Collect,
    Playback,
}

#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct SfxRequest {
    pub cue: SfxCue,
}

impl SfxRequest {
    pub(crate) fn new(cue: SfxCue) -> Self {
        Self { cue }
    }
}

#[derive(Component)]
pub(crate) struct UiSoundVoice {
    pub cue: SfxCue,
    pub started_at: f64,
    current_volume: f32,
}

#[derive(Resource, Default)]
pub(crate) struct SfxStats {
    pub started: [u64; 7],
    pub coalesced: u64,
    pub unavailable: u64,
    pub muted: u64,
    pub budget_dropped: u64,
    pub expired_pending: u64,
}

#[derive(Resource)]
struct UiPlaybackState {
    last_started: f64,
}

impl Default for UiPlaybackState {
    fn default() -> Self {
        Self {
            last_started: f64::NEG_INFINITY,
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<SfxAssets>()
        .init_resource::<SfxStats>()
        .init_resource::<UiPlaybackState>()
        .add_message::<SfxRequest>()
        .configure_sets(
            PostUpdate,
            (SfxSet::Collect, SfxSet::Playback)
                .chain()
                .before(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(PostUpdate, play_ui.in_set(SfxSet::Playback));
}

fn prefer(previous: Option<SfxCue>, next: SfxCue) -> Option<SfxCue> {
    if next == SfxCue::CartRoll {
        return previous;
    }
    if previous.is_none_or(|cue| next.priority() > cue.priority()) {
        Some(next)
    } else {
        previous
    }
}

fn play_ui(
    mut commands: Commands,
    mut requests: MessageReader<SfxRequest>,
    mut bank: ResMut<SfxAssets>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    time: Res<Time<Real>>,
    mut state: ResMut<UiPlaybackState>,
    mut stats: ResMut<SfxStats>,
    mut voices: Query<(
        Entity,
        &mut UiSoundVoice,
        &mut PlaybackSettings,
        Option<&mut AudioSink>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let gain = settings.effects_gain();
    bank.report_failures(&assets);
    let mut occupied = 0;
    for (entity, mut voice, mut initial, sink) in &mut voices {
        let expired = sink.is_none() && now - voice.started_at > PENDING_TIMEOUT;
        if gain <= 0.0
            || expired
            || sink.as_ref().is_some_and(|sink| sink.empty())
            || now - voice.started_at > 4.0
        {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            stats.expired_pending += u64::from(expired);
        } else {
            occupied += 1;
            let volume = Volume::Linear(voice.cue.gain() * gain);
            initial.volume = volume;
            if let Some(mut sink) = sink {
                let target = voice.cue.gain() * gain;
                let difference = target - voice.current_volume;
                if difference.abs() > 0.0001 {
                    voice.current_volume += difference * (time.delta_secs() / 0.06).clamp(0.0, 1.0);
                    sink.set_volume(Volume::Linear(voice.current_volume));
                }
            }
        }
    }
    let mut cue = None;
    let mut count: u64 = 0;
    for request in requests.read() {
        if request.cue != SfxCue::CartRoll {
            count += 1;
            cue = prefer(cue, request.cue);
        }
    }
    let Some(cue) = cue else {
        return;
    };
    stats.coalesced += count.saturating_sub(1);
    if gain <= 0.0 {
        stats.muted += 1;
        return;
    }
    if occupied >= MAX_UI_VOICES || now - state.last_started < START_INTERVAL {
        stats.budget_dropped += 1;
        return;
    }
    let Some(handle) = bank.ready_handle(cue, &assets) else {
        stats.unavailable += 1;
        return;
    };
    commands.spawn((
        Name::new(format!("UI sound: {cue:?}")),
        UiSoundVoice {
            cue,
            started_at: now,
            current_volume: cue.gain() * gain,
        },
        AudioPlayer::new(handle),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(cue.gain() * gain)),
    ));
    state.last_started = now;
    stats.started[cue as usize] += 1;
}

#[cfg(test)]
mod tests;
