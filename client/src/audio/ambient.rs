//! ambient systems.

use super::*;

/// Ensure the ambient audio entity exists (spawn once when playing).
/// We start it paused; we will play/pause via `AudioSink` based on walking/biome.
pub fn ensure_ambient_entity(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    mut audio_state: ResMut<AudioState>,
    existing: Query<Entity, With<AmbientSound>>,
) {
    // Don't spawn until assets are ready
    if !audio_state.assets_ready {
        return;
    }

    if audio_state.ambient_spawned || !existing.is_empty() {
        return;
    }

    let Some(audio) = audio else { return };

    info!("Spawning desert ambient audio entity...");
    commands.spawn((
        AmbientSound,
        AudioPlayer::new(audio.desert_ambient.clone()),
        PlaybackSettings::LOOP
            .paused()
            .with_volume(Volume::Linear(0.4)),
    ));

    audio_state.ambient_spawned = true;
}

/// Control desert walking ambient:
/// - Only plays while walking (WASD pressed)
/// - Only plays in Desert biome
/// - Pauses otherwise
pub fn update_desert_walking_ambient(
    input_state: Res<InputState>,
    terrain: Res<WorldTerrain>,
    player_pos: Query<&PlayerPosition, With<LocalPlayer>>,
    ambient: Query<&AudioSink, With<AmbientSound>>,
) {
    let Ok(pos) = player_pos.single() else { return };

    let biome = terrain.get_biome(pos.0.x, pos.0.z);
    let walking =
        input_state.forward || input_state.backward || input_state.left || input_state.right;
    let should_play = walking && biome == Biome::Desert;

    for sink in ambient.iter() {
        if should_play && sink.is_paused() {
            sink.play();
        } else if !should_play && !sink.is_paused() {
            sink.pause();
        }
    }
}

/// Stop ambient sounds when leaving gameplay
pub fn cleanup_ambient_sounds(
    mut commands: Commands,
    mut audio_state: ResMut<AudioState>,
    ambient_sounds: Query<Entity, With<AmbientSound>>,
) {
    for entity in ambient_sounds.iter() {
        commands.entity(entity).despawn();
    }
    audio_state.ambient_spawned = false;
}

/// Stop/despawn remote looped spatial sounds (footsteps).
pub fn cleanup_remote_loop_sounds(
    mut commands: Commands,
    mut emitter_index: ResMut<RemoteAudioEmitterIndex>,
    footsteps: Query<Entity, With<RemoteFootstepEmitter>>,
) {
    for e in footsteps.iter() {
        commands.entity(e).despawn();
    }
    emitter_index.footstep_targets.clear();
    emitter_index.footstep_by_emitter.clear();
}
