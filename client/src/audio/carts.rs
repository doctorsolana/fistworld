//! Bounded, ground-focused rolling audio for observed physical porter carts.
//! The stable character owns a voice; replacing its render rig never restarts it.

use super::filtered::{FilterControl, FilteredSound, WorldSoundCache};
use super::perspective::{CART, LISTENER_EAR_GAP};
use super::sfx::{SfxAssets, SfxCue, SfxSet};
use super::AudioSettings;
use crate::{camera_rts::CommanderCamera, hero::HeroVisual, states::GameState};
use bevy::{
    audio::{SpatialAudioSink, SpatialListener, SpatialScale, Volume},
    prelude::*,
};
use shared::{
    components::{AboardBoat, CharacterActivity, CharacterKind, CharacterMotion, Mounted},
    economy::PorterCartState,
    terrain::WorldTerrain,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

const VOICE_LIMIT: usize = 4;
const SCAN_SECONDS: f64 = 0.1;
const HEARING_RADIUS: f32 = CART.hearing_radius;
const REFERENCE_DISTANCE: f32 = CART.reference_distance;
const MIN_SPEED: f32 = 0.24;
const MAX_MOVEMENT_SAMPLE: f32 = 32.0;
const MIN_AUDIBLE_GAIN: f32 = 0.002;
const FADE_SECONDS: f32 = 0.3;
const PENDING_TIMEOUT: f64 = 2.0;
const RETRY_SECONDS: f64 = 5.0;

/// Marker for the sole spatial listener, independent of camera height and tilt.
#[derive(Component)]
pub(crate) struct CartAudioListener;

/// Actual allocated voice, including its brief fade or pending device start.
/// Diagnostics can inspect this without reaching into render-rig internals.
#[derive(Component)]
pub(crate) struct CartRollVoice {
    pub(crate) owner: Entity,
    /// Final sink gain; native spatial attenuation is applied once afterward.
    pub(crate) gain: f32,
    /// Narrow playback multiplier, never simulation time warp.
    pub(crate) speed: f32,
    pub(crate) cutoff_hz: f32,
    pub(crate) estimated_gain: f32,
    filter: FilterControl,
    born: f64,
    last_position: Vec3,
    last_moved: f64,
    envelope: f32,
}

#[derive(Resource, Default)]
pub(crate) struct CartAudioMetrics {
    pub(crate) candidates: usize,
    pub(crate) active: usize,
    pub(crate) pending: usize,
    pub(crate) starts: u64,
    pub(crate) stops: u64,
    pub(crate) timed_out: u64,
}

#[derive(Resource, Default)]
struct ListeningFrame {
    position: Vec3,
    detail: f32,
    zoom: f32,
    valid: bool,
}

#[derive(Clone, Copy)]
struct Candidate {
    owner: Entity,
    position: Vec3,
    speed: f32,
    score: f32,
}

struct MovementSample {
    position: Vec3,
    at: f64,
}

#[derive(Resource, Default)]
struct CartAudioState {
    samples: HashMap<Entity, MovementSample>,
    desired: [Option<Candidate>; VOICE_LIMIT],
    last_scan: f64,
    retry_at: f64,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<CartAudioState>()
        .init_resource::<CartAudioMetrics>()
        .init_resource::<ListeningFrame>()
        .add_systems(
            PostUpdate,
            (update_listener, collect_carts)
                .chain()
                .in_set(SfxSet::Collect)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            PostUpdate,
            update_voices
                .in_set(SfxSet::Playback)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(OnExit(GameState::Playing), stop_carts);
}

fn update_listener(
    mut commands: Commands,
    time: Res<Time<Real>>,
    terrain: Option<Res<WorldTerrain>>,
    cameras: Query<&CommanderCamera>,
    mut frame: ResMut<ListeningFrame>,
    mut listeners: Query<&mut Transform, With<CartAudioListener>>,
) {
    let Ok(camera) = cameras.single() else {
        frame.valid = false;
        return;
    };
    frame.valid = true;
    let mut position = camera.focus;
    if let Some(terrain) = terrain {
        position.y = terrain
            .get_water_height(position.x, position.z)
            .unwrap_or_else(|| terrain.get_height(position.x, position.z));
    }
    position.y += 1.2;
    frame.position = position;
    frame.zoom = camera.zoom;
    frame.detail = approach(
        frame.detail,
        zoom_gain(camera.zoom),
        time.delta_secs(),
        0.75,
    );
    let transform =
        Transform::from_translation(position).with_rotation(Quat::from_rotation_y(camera.yaw));
    if let Ok(mut listener) = listeners.single_mut() {
        *listener = transform;
    } else {
        commands.spawn((
            Name::new("Ground-focused world audio listener"),
            CartAudioListener,
            cart_spatial_listener(),
            transform,
        ));
    }
}

type CartActors<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static HeroVisual,
        Option<&'static CharacterActivity>,
        Option<&'static CharacterMotion>,
        Has<AboardBoat>,
        Has<Mounted>,
    ),
    (
        With<CharacterKind>,
        With<PorterCartState>,
        Without<CartRollVoice>,
    ),
>;

#[allow(clippy::too_many_arguments)]
fn collect_carts(
    time: Res<Time<Real>>,
    settings: Res<AudioSettings>,
    frame: Res<ListeningFrame>,
    terrain: Option<Res<WorldTerrain>>,
    actors: CartActors,
    voices: Query<&CartRollVoice>,
    mut state: ResMut<CartAudioState>,
    mut metrics: ResMut<CartAudioMetrics>,
) {
    let effects_gain = settings.effects_gain();
    if effects_gain == 0.0 || !frame.valid || frame.detail < MIN_AUDIBLE_GAIN {
        state.samples.clear();
        state.desired.fill(None);
        metrics.candidates = 0;
        return;
    }
    let now = time.elapsed_secs_f64();
    if now - state.last_scan < SCAN_SECONDS {
        return;
    }
    state.last_scan = now;
    state.desired.fill(None);
    metrics.candidates = 0;
    // This 10 Hz query visits streamed physical carts only, never the population.
    // Sample storage is reused; ranking has exactly four stack-backed slots.
    for (owner, transform, visual, activity, motion, aboard, mounted) in &actors {
        let position = transform.translation;
        let distance = ground_distance(position, frame.position);
        if distance >= HEARING_RADIUS
            || !eligible(activity, motion, aboard, mounted)
            || terrain.as_ref().is_some_and(|terrain| {
                shared::character::locomotion::swimming_at(
                    terrain.get_height(position.x, position.z),
                    terrain.get_water_height(position.x, position.z),
                    position.y,
                    aboard,
                )
            })
        {
            continue;
        }
        let sample = state
            .samples
            .entry(owner)
            .or_insert(MovementSample { position, at: now });
        let movement = ground_distance(position, sample.position);
        let elapsed = now - sample.at;
        sample.position = position;
        sample.at = now;
        if !measured_movement(movement, visual.speed(), elapsed) {
            continue;
        }
        let gain = source_gain(distance, frame.detail, visual.speed()) * effects_gain;
        // Estimate the backend falloff for ranking only. Playback receives gain
        // without this inverse square, avoiding doubled distance attenuation.
        let estimate = gain * native_distance_estimate(distance);
        if estimate < MIN_AUDIBLE_GAIN {
            continue;
        }
        metrics.candidates += 1;
        let retained = voices.iter().any(|voice| voice.owner == owner);
        insert_candidate(
            &mut state.desired,
            Candidate {
                owner,
                position,
                speed: visual.speed(),
                score: estimate * if retained { 1.15 } else { 1.0 },
            },
        );
    }
    // Unobserved, distant, indoor or otherwise suppressed carts retain no state.
    state.samples.retain(|_, sample| sample.at == now);
}

#[allow(clippy::too_many_arguments)]
fn update_voices(
    mut commands: Commands,
    time: Res<Time<Real>>,
    settings: Res<AudioSettings>,
    frame: Res<ListeningFrame>,
    assets: Res<AssetServer>,
    bank: Res<SfxAssets>,
    raw_audio: Res<Assets<AudioSource>>,
    mut cache: ResMut<WorldSoundCache>,
    mut filtered_audio: ResMut<Assets<FilteredSound>>,
    actors: CartActors,
    mut state: ResMut<CartAudioState>,
    mut metrics: ResMut<CartAudioMetrics>,
    mut voices: Query<(
        Entity,
        &mut CartRollVoice,
        &mut Transform,
        &mut PlaybackSettings,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    let handle = bank.ready_handle(SfxCue::CartRoll, &assets);
    let effects_gain = settings.effects_gain();
    let enabled = effects_gain > 0.0 && frame.valid && handle.is_some();
    // Preparation happens once on the compute pool, only when an audible mover
    // needs the sound. Existing voices retain their shared PCM independently.
    let clip = if enabled && state.desired.iter().any(Option::is_some) {
        handle
            .as_ref()
            .and_then(|handle| cache.prepare(handle, &raw_audio))
    } else {
        None
    };
    let mut occupied = [None; VOICE_LIMIT];
    let mut count = 0;
    metrics.active = 0;
    metrics.pending = 0;
    for (entity, mut voice, mut transform, mut initial, sink) in &mut voices {
        let actor = actors.get(voice.owner).ok();
        let timed_out = sink.is_none() && now - voice.born >= PENDING_TIMEOUT;
        // Source loss and mute end pending and active voices immediately. A
        // timeout also backs off starts when Bevy has no output device.
        if !enabled
            || actor.is_none()
            || timed_out
            || sink.as_ref().is_some_and(|sink| sink.empty())
        {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            metrics.stops += 1;
            if timed_out {
                metrics.timed_out += 1;
                state.retry_at = now + RETRY_SECONDS;
            }
            continue;
        }
        let (_, owner_transform, visual, activity, motion, aboard, mounted) = actor.unwrap();
        let position = owner_transform.translation;
        let moved = ground_distance(position, voice.last_position);
        voice.last_position = position;
        transform.translation = position + Vec3::Y * 0.45;
        if moved > 0.0001 && moved <= MAX_MOVEMENT_SAMPLE && visual.speed() >= MIN_SPEED {
            voice.last_moved = now;
        } else if moved > MAX_MOVEMENT_SAMPLE {
            // Do not let the short movement grace carry sound across a snap.
            voice.last_moved = f64::NEG_INFINITY;
        }
        let desired = state
            .desired
            .iter()
            .flatten()
            .any(|candidate| candidate.owner == voice.owner);
        let moving = desired
            && eligible(activity, motion, aboard, mounted)
            && moved <= MAX_MOVEMENT_SAMPLE
            && now - voice.last_moved <= 0.15;
        voice.envelope = approach(
            voice.envelope,
            if moving { 1.0 } else { 0.0 },
            dt,
            FADE_SECONDS,
        );
        let intended_gain = source_gain(
            ground_distance(position, frame.position),
            frame.detail,
            visual.speed(),
        ) * voice.envelope
            * effects_gain;
        voice.gain = approach(voice.gain, intended_gain, dt, 0.1);
        voice.cutoff_hz = CART.cutoff_hz(ground_distance(position, frame.position), frame.zoom);
        voice.filter.set_cutoff(voice.cutoff_hz);
        voice.estimated_gain =
            voice.gain * native_distance_estimate(ground_distance(position, frame.position));
        voice.speed = approach(voice.speed, playback_speed(visual.speed()), dt, 0.3);
        if !moving && voice.envelope == 0.0
            || voice.gain < MIN_AUDIBLE_GAIN && frame.detail < MIN_AUDIBLE_GAIN
        {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            metrics.stops += 1;
            continue;
        }
        initial.volume = Volume::Linear(voice.gain);
        initial.speed = voice.speed;
        if let Some(mut sink) = sink {
            sink.set_volume(initial.volume);
            sink.set_speed(voice.speed);
            metrics.active += 1;
        } else {
            metrics.pending += 1;
        }
        if count < VOICE_LIMIT {
            occupied[count] = Some(voice.owner);
        }
        count += 1;
    }
    if !enabled || clip.is_none() || now < state.retry_at || count >= VOICE_LIMIT {
        return;
    }
    for candidate in state.desired.iter().flatten() {
        if count >= VOICE_LIMIT {
            break;
        }
        if occupied.contains(&Some(candidate.owner)) {
            continue;
        }
        // Refresh authoritative presence before admitting cached 10 Hz intent.
        let Ok((_, transform, visual, activity, motion, aboard, mounted)) =
            actors.get(candidate.owner)
        else {
            continue;
        };
        if !eligible(activity, motion, aboard, mounted) || visual.speed() < MIN_SPEED {
            continue;
        }
        let position = transform.translation;
        // A correction since the scan must not start a fresh loop elsewhere.
        if ground_distance(position, candidate.position) > MAX_MOVEMENT_SAMPLE {
            continue;
        }
        let speed = playback_speed(candidate.speed);
        let cutoff_hz = CART.cutoff_hz(ground_distance(position, frame.position), frame.zoom);
        let (source, filter) = FilteredSound::new(clip.as_ref().unwrap().clone(), true, cutoff_hz);
        commands.spawn((
            Name::new("Porter cart rolling"),
            CartRollVoice {
                owner: candidate.owner,
                gain: 0.0,
                speed,
                cutoff_hz,
                estimated_gain: 0.0,
                filter,
                born: now,
                last_position: position,
                last_moved: now,
                envelope: 0.0,
            },
            AudioPlayer(filtered_audio.add(source)),
            // The filtered decoder loops internally so its coefficients and
            // filter history keep evolving across every seam.
            PlaybackSettings::ONCE
                .with_volume(Volume::Linear(0.0))
                .with_speed(speed)
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(1.0 / REFERENCE_DISTANCE)),
            Transform::from_translation(position + Vec3::Y * 0.45),
        ));
        occupied[count] = Some(candidate.owner);
        count += 1;
        metrics.pending += 1;
        metrics.starts += 1;
    }
}

fn stop_carts(
    mut commands: Commands,
    voices: Query<(Entity, Option<&SpatialAudioSink>), With<CartRollVoice>>,
    listeners: Query<Entity, With<CartAudioListener>>,
    mut state: ResMut<CartAudioState>,
    mut frame: ResMut<ListeningFrame>,
    mut metrics: ResMut<CartAudioMetrics>,
) {
    for (entity, sink) in &voices {
        if let Some(sink) = sink {
            sink.stop();
        }
        commands.entity(entity).despawn();
        metrics.stops += 1;
    }
    for entity in &listeners {
        commands.entity(entity).despawn();
    }
    *state = CartAudioState::default();
    *frame = ListeningFrame::default();
    metrics.active = 0;
    metrics.pending = 0;
    metrics.candidates = 0;
}

fn eligible(
    activity: Option<&CharacterActivity>,
    motion: Option<&CharacterMotion>,
    aboard: bool,
    mounted: bool,
) -> bool {
    !aboard
        && !mounted
        && !matches!(
            activity,
            Some(
                CharacterActivity::Indoors
                    | CharacterActivity::Sitting
                    | CharacterActivity::LyingDown
            )
        )
        && motion.is_none_or(|motion| motion.is_moving())
}

fn measured_movement(distance: f32, speed: f32, elapsed: f64) -> bool {
    elapsed > 0.0
        && elapsed <= 0.5
        && distance > 0.005
        && distance <= MAX_MOVEMENT_SAMPLE
        && speed.is_finite()
        && speed >= MIN_SPEED
}

fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    (a.xz() - b.xz()).length()
}

fn cart_spatial_listener() -> SpatialListener {
    // Rodio 0.22.2's directional factor strengthens the farther ear. With our
    // scaled gap <= 1/3, it reverses stereo at EVERY off-centre source position,
    // even after its distance attenuation. Exchange ear positions to correct
    // the channels while preserving total distance gain. This is specific to
    // the pinned native backend and scale: keep the stereo regression when
    // upgrading Rodio or introducing other spatial cue reference distances.
    // The ground listener transform still follows the camera's ordinary yaw.
    SpatialListener {
        left_ear_offset: Vec3::X * LISTENER_EAR_GAP / 2.0,
        right_ear_offset: -Vec3::X * LISTENER_EAR_GAP / 2.0,
    }
}

fn zoom_gain(zoom: f32) -> f32 {
    CART.zoom_gain(zoom)
}

fn source_gain(distance: f32, detail: f32, speed: f32) -> f32 {
    SfxCue::CartRoll.gain() * detail * CART.edge_gain(distance) * (speed / 2.0).clamp(0.35, 1.0)
}

fn native_distance_estimate(distance: f32) -> f32 {
    CART.native_gain_estimate(distance)
}

fn playback_speed(speed: f32) -> f32 {
    (0.94 + speed * 0.03).clamp(0.94, 1.06)
}

fn approach(current: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    current + (target - current).clamp(-dt.max(0.0) / seconds, dt.max(0.0) / seconds)
}

fn insert_candidate(ranked: &mut [Option<Candidate>; VOICE_LIMIT], candidate: Candidate) {
    let Some(index) = ranked.iter().position(|slot| {
        slot.is_none_or(|old| {
            candidate.score > old.score
                || candidate.score == old.score && candidate.owner.to_bits() < old.owner.to_bits()
        })
    }) else {
        return;
    };
    for shift in (index + 1..VOICE_LIMIT).rev() {
        ranked[shift] = ranked[shift - 1];
    }
    ranked[index] = Some(candidate);
}
