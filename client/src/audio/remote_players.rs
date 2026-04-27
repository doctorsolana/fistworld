//! remote players systems.

use super::*;

/// Play gunshot sound when we *actually fire* (check ShootingState resource).
/// Handle audio events from remote players (spatial audio)
///
/// When other players shoot, we receive an AudioEvent from the server
/// and play a spatial sound at their position.
/// Respects max_remote_combat limit by despawning oldest sounds when at capacity.
pub fn handle_remote_audio_events(
    mut commands: Commands,
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    audio_manager: Res<AudioManager>,
    camera: Query<&Transform, With<Camera3d>>,
    // Get our local player ID to skip our own sounds (we already play them locally)
    local_player: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    // Receive network messages
    mut receiver: Query<
        &mut MessageReceiver<AudioEvent>,
        (With<crate::GameClient>, With<Connected>),
    >,
    // Query existing remote combat sounds to enforce limit
    remote_sounds: Query<(Entity, &ManagedAudioTag, &Transform), With<RemoteSpatialSound>>,
) {
    // Don't process until audio assets are ready
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let now = time.elapsed_secs();

    // Get our peer ID to skip our own sounds
    let our_id = local_player.iter().next().map(|id| peer_id_to_u64(id.0));

    // Batch incoming gunshots this frame.
    let mut pending_gunshots: Vec<(shared::weapons::WeaponType, Vec3)> = Vec::new();
    for mut recv in receiver.iter_mut() {
        for audio_event in recv.receive() {
            // Skip if this is our own sound (we already play it locally)
            if Some(audio_event.player_id) == our_id {
                continue;
            }

            match audio_event.kind {
                AudioEventKind::Gunshot { weapon_type } => {
                    pending_gunshots.push((weapon_type, audio_event.position));
                }
            }
        }
    }

    if pending_gunshots.is_empty() {
        return;
    }

    let mut current_remote_count = remote_sounds
        .iter()
        .filter(|(_, tag, _)| tag.priority == AudioPriority::CombatRemote)
        .count();

    let needed_capacity = current_remote_count.saturating_add(pending_gunshots.len());
    if needed_capacity > audio_manager.max_remote_combat {
        let to_remove = needed_capacity - audio_manager.max_remote_combat;
        let mut eviction_candidates: Vec<(Entity, f32)> = remote_sounds
            .iter()
            .filter(|(_, tag, _)| tag.priority == AudioPriority::CombatRemote)
            .map(|(entity, tag, tf)| {
                // Lower score = older/farther, evict first.
                let score = tag.spawn_time - tf.translation.distance_squared(listener_pos) * 0.001;
                (entity, score)
            })
            .collect();

        eviction_candidates
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let evict_count = to_remove.min(eviction_candidates.len());
        for (entity, _) in eviction_candidates.into_iter().take(evict_count) {
            commands.entity(entity).despawn();
        }
        current_remote_count = current_remote_count.saturating_sub(evict_count);
    }

    let available_slots = audio_manager
        .max_remote_combat
        .saturating_sub(current_remote_count);
    if available_slots == 0 {
        return;
    }
    if pending_gunshots.len() > available_slots {
        pending_gunshots.sort_by(|a, b| {
            a.1.distance_squared(listener_pos)
                .partial_cmp(&b.1.distance_squared(listener_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pending_gunshots.truncate(available_slots);
    }

    for (weapon_type, position) in pending_gunshots {
        // Random pitch variation for variety (±5%)
        let pitch = 0.95 + rand::random::<f32>() * 0.1;

        let sound = match weapon_type {
            shared::weapons::WeaponType::Shotgun => audio.shotgun_shot.clone(),
            shared::weapons::WeaponType::Sniper => audio.sniper_shot.clone(),
            shared::weapons::WeaponType::Pistol => audio.revolver_shot.clone(),
            shared::weapons::WeaponType::AssaultRifle => audio.assault_shot.clone(),
            shared::weapons::WeaponType::Unarmed => audio.assault_shot.clone(),
        };

        commands.spawn((
            RemoteSpatialSound,
            ManagedAudioTag {
                priority: AudioPriority::CombatRemote,
                spawn_time: now,
            },
            AudioPlayer::new(sound),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(0.8))
                .with_speed(pitch)
                .with_spatial(true),
            Transform::from_translation(position),
        ));
    }
}

/// Ensure we have spatial footstep emitters for nearby remote players + NPCs.
///
/// Perf notes:
/// - No network traffic: we infer movement from replicated transforms.
/// - We only spawn emitters within a distance threshold.
/// - Respects max_remote_footsteps limit, prioritizing closest entities.
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
