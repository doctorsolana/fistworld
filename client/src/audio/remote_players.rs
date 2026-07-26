//! remote players systems.

use super::*;

/// Play gunshot sound when we *actually fire* (check ShootingState resource).
/// Handle audio events from remote players (spatial audio)
///
/// When other players shoot, we receive an AudioEvent from the server
/// and play a spatial sound at their position.
/// Respects max_remote_combat limit by despawning oldest sounds when at capacity.
pub fn ensure_remote_footstep_emitters(
    mut commands: Commands,
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    audio_manager: Res<AudioManager>,
    mut emitter_index: ResMut<RemoteAudioEmitterIndex>,
    camera: Query<&Transform, With<Camera3d>>,
    // Remote players only (local player has their own loop)
    players: Query<(Entity, &Player, &Transform), (With<Player>, Without<LocalPlayer>)>,
    // NPCs
    npcs: Query<(Entity, &Transform), With<Npc>>,
    // Vehicles to identify which players are driving (no footsteps)
    vehicles: Query<&VehicleDriver, With<Vehicle>>,
    mut driving_ids_cache: Local<HashSet<u64>>,
    mut candidate_cache: Local<Vec<(Entity, Vec3, f32)>>,
) {
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let now = time.elapsed_secs();

    let current_count = emitter_index.footstep_targets.len();
    if current_count >= audio_manager.max_remote_footsteps {
        return;
    }
    let available_slots = audio_manager.max_remote_footsteps - current_count;

    driving_ids_cache.clear();
    for driver in vehicles.iter() {
        if let Some(id) = driver.driver_id {
            driving_ids_cache.insert(id);
        }
    }

    candidate_cache.clear();
    let max_dist_sq = REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE * REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE;

    for (entity, player, transform) in players.iter() {
        let player_id = peer_id_to_u64(player.client_id);
        if driving_ids_cache.contains(&player_id)
            || emitter_index.footstep_targets.contains(&entity)
        {
            continue;
        }

        let dist_sq = transform.translation.distance_squared(listener_pos);
        if dist_sq <= max_dist_sq {
            candidate_cache.push((entity, transform.translation, dist_sq));
        }
    }

    for (entity, transform) in npcs.iter() {
        if emitter_index.footstep_targets.contains(&entity) {
            continue;
        }

        let dist_sq = transform.translation.distance_squared(listener_pos);
        if dist_sq <= max_dist_sq {
            candidate_cache.push((entity, transform.translation, dist_sq));
        }
    }

    if candidate_cache.len() > available_slots {
        candidate_cache.select_nth_unstable_by(available_slots, |a, b| a.2.total_cmp(&b.2));
        candidate_cache.truncate(available_slots);
    }
    candidate_cache.sort_by(|a, b| a.2.total_cmp(&b.2));

    for (entity, pos, _dist_sq) in candidate_cache.iter().copied() {
        let emitter_entity = commands
            .spawn((
                RemoteFootstepEmitter { target: entity },
                RemoteFootstepState {
                    last_pos: pos,
                    playing: false,
                },
                ManagedAudioTag {
                    priority: AudioPriority::Ambient,
                    spawn_time: now,
                },
                AudioPlayer::new(audio.desert_ambient.clone()),
                PlaybackSettings::LOOP
                    .paused()
                    .with_volume(Volume::Linear(REMOTE_FOOTSTEP_VOLUME))
                    .with_spatial(true),
                Transform::from_translation(pos),
                GlobalTransform::default(),
            ))
            .id();
        emitter_index.footstep_targets.insert(entity);
        emitter_index
            .footstep_by_emitter
            .insert(emitter_entity, entity);
    }
}

/// Update remote footstep emitters:
/// - Follow the target entity
/// - Pause/play the loop based on inferred movement (with hysteresis)
pub fn update_remote_footstep_emitters(
    mut commands: Commands,
    time: Res<Time>,
    mut emitter_index: ResMut<RemoteAudioEmitterIndex>,
    camera: Query<&Transform, (With<Camera3d>, Without<RemoteFootstepEmitter>)>,
    target_transforms: Query<&Transform, (Without<RemoteFootstepEmitter>, Without<Camera3d>)>,
    mut emitters: Query<
        (
            Entity,
            &RemoteFootstepEmitter,
            &mut RemoteFootstepState,
            &mut Transform,
            &SpatialAudioSink,
        ),
        Without<Camera3d>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let despawn_dist_sq = REMOTE_FOOTSTEP_DESPAWN_DISTANCE * REMOTE_FOOTSTEP_DESPAWN_DISTANCE;

    for (entity, emitter, mut state, mut transform, sink) in emitters.iter_mut() {
        let Ok(target_tf) = target_transforms.get(emitter.target) else {
            // Target despawned.
            emitter_index.footstep_by_emitter.remove(&entity);
            emitter_index.footstep_targets.remove(&emitter.target);
            commands.entity(entity).despawn();
            continue;
        };

        let target_pos = target_tf.translation;

        // Cull far emitters to keep entity count and audio sinks small.
        if target_pos.distance_squared(listener_pos) > despawn_dist_sq {
            emitter_index.footstep_by_emitter.remove(&entity);
            emitter_index.footstep_targets.remove(&emitter.target);
            commands.entity(entity).despawn();
            continue;
        }

        // Follow.
        transform.translation = target_pos;

        // Infer movement.
        let delta = target_pos - state.last_pos;
        state.last_pos = target_pos;

        let horizontal = Vec3::new(delta.x, 0.0, delta.z);
        let speed = horizontal.length() / dt;

        let should_play = if state.playing {
            speed > REMOTE_FOOTSTEP_STOP_SPEED
        } else {
            speed > REMOTE_FOOTSTEP_START_SPEED
        };

        if should_play && sink.is_paused() {
            sink.play();
            state.playing = true;
        } else if !should_play && !sink.is_paused() {
            sink.pause();
            state.playing = false;
        }
    }
}
