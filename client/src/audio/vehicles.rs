//! vehicles systems.

use super::*;

/// Ensure spatial vehicle audio emitters exist for nearby *remote* vehicles.
///
/// We don't rely on network audio events for engines: we already replicate `VehicleState`,
/// so we can generate continuous audio locally (lower bandwidth, more robust).
pub fn ensure_remote_vehicle_audio_emitters(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    mut emitter_index: ResMut<RemoteAudioEmitterIndex>,
    camera: Query<&Transform, With<Camera3d>>,
    // Our peer ID (so we can skip the vehicle we're driving; local has non-spatial loops)
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(Entity, &VehicleDriver, &VehicleState, &Transform), With<Vehicle>>,
) {
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let max_spawn_distance_sq =
        REMOTE_VEHICLE_MAX_SPAWN_DISTANCE * REMOTE_VEHICLE_MAX_SPAWN_DISTANCE;

    let our_id = client_query.iter().next().map(|id| peer_id_to_u64(id.0));

    for (veh_entity, driver, _state, veh_tf) in vehicles.iter() {
        // Skip the vehicle we're driving (local audio handles it).
        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                continue;
            }
        }

        if veh_tf.translation.distance_squared(listener_pos) > max_spawn_distance_sq {
            continue;
        }

        if !emitter_index.vehicle_idle_targets.contains(&veh_entity) {
            let emitter_entity = commands
                .spawn((
                    RemoteVehicleIdleSound {
                        vehicle: veh_entity,
                    },
                    AudioPlayer::new(audio.hover_idle.clone()),
                    PlaybackSettings::LOOP
                        .with_volume(Volume::Linear(0.0))
                        .with_spatial(true),
                    Transform::from_translation(veh_tf.translation),
                    GlobalTransform::default(),
                ))
                .id();
            emitter_index.vehicle_idle_targets.insert(veh_entity);
            emitter_index
                .vehicle_idle_by_emitter
                .insert(emitter_entity, veh_entity);
        }

        if !emitter_index.vehicle_cruise_targets.contains(&veh_entity) {
            let emitter_entity = commands
                .spawn((
                    RemoteVehicleCruiseSound {
                        vehicle: veh_entity,
                    },
                    AudioPlayer::new(audio.bike_cruise.clone()),
                    PlaybackSettings::LOOP
                        .with_volume(Volume::Linear(0.0))
                        .with_spatial(true),
                    Transform::from_translation(veh_tf.translation),
                    GlobalTransform::default(),
                ))
                .id();
            emitter_index.vehicle_cruise_targets.insert(veh_entity);
            emitter_index
                .vehicle_cruise_by_emitter
                .insert(emitter_entity, veh_entity);
        }
    }
}

/// Compute the crossfade + pitch parameters for the motorbike audio based on speed.
fn vehicle_audio_params(speed: f32) -> (f32, f32, f32, f32) {
    // Max speed from vehicle constants (~45 m/s)
    const MAX_SPEED: f32 = 45.0;
    let speed_ratio = (speed / MAX_SPEED).clamp(0.0, 1.0);

    // === CROSSFADE LOGIC ===
    // Idle: full volume at 0 speed, fades out by ~30% max speed
    // Cruise: silent at 0, fades in from ~10% to full at ~40% max speed
    let idle_volume = if speed_ratio < 0.3 {
        1.0 - (speed_ratio / 0.3)
    } else {
        0.0
    };

    let cruise_volume = if speed_ratio < 0.1 {
        0.0
    } else if speed_ratio < 0.4 {
        (speed_ratio - 0.1) / 0.3
    } else {
        1.0
    };

    // === PITCH MODULATION ===
    // Idle: subtle pitch variation 0.9x to 1.05x
    // Cruise: 0.85x at slow speeds up to 1.25x at max speed
    let idle_pitch = 0.9 + speed_ratio * 0.15;
    let cruise_pitch = 0.85 + speed_ratio * 0.4;

    (idle_volume, cruise_volume, idle_pitch, cruise_pitch)
}

/// Update remote vehicle audio emitters:
/// - Follow the vehicle transform
/// - Modulate volume/pitch based on speed
/// - Despawn when far away or when we become the driver (avoid double audio)
pub fn update_remote_vehicle_audio_emitters(
    mut commands: Commands,
    mut emitter_index: ResMut<RemoteAudioEmitterIndex>,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<RemoteVehicleIdleSound>,
            Without<RemoteVehicleCruiseSound>,
        ),
    >,
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<
        (&VehicleDriver, &VehicleState, &Transform),
        (
            With<Vehicle>,
            Without<RemoteVehicleIdleSound>,
            Without<RemoteVehicleCruiseSound>,
        ),
    >,
    mut idle_emitters: Query<
        (
            Entity,
            &RemoteVehicleIdleSound,
            &mut Transform,
            &mut SpatialAudioSink,
        ),
        Without<RemoteVehicleCruiseSound>,
    >,
    mut cruise_emitters: Query<
        (
            Entity,
            &RemoteVehicleCruiseSound,
            &mut Transform,
            &mut SpatialAudioSink,
        ),
        Without<RemoteVehicleIdleSound>,
    >,
) {
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let despawn_distance_sq = REMOTE_VEHICLE_DESPAWN_DISTANCE * REMOTE_VEHICLE_DESPAWN_DISTANCE;

    let our_id = client_query.iter().next().map(|id| peer_id_to_u64(id.0));

    // Update idle loops
    for (entity, e, mut tf, mut sink) in idle_emitters.iter_mut() {
        let Ok((driver, state, veh_tf)) = vehicles.get(e.vehicle) else {
            emitter_index.vehicle_idle_by_emitter.remove(&entity);
            emitter_index.vehicle_idle_targets.remove(&e.vehicle);
            commands.entity(entity).despawn();
            continue;
        };

        // If we started driving this vehicle, remove remote audio emitters.
        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                emitter_index.vehicle_idle_by_emitter.remove(&entity);
                emitter_index.vehicle_idle_targets.remove(&e.vehicle);
                commands.entity(entity).despawn();
                continue;
            }
        }

        let pos = veh_tf.translation;
        if pos.distance_squared(listener_pos) > despawn_distance_sq {
            emitter_index.vehicle_idle_by_emitter.remove(&entity);
            emitter_index.vehicle_idle_targets.remove(&e.vehicle);
            commands.entity(entity).despawn();
            continue;
        }

        tf.translation = pos;

        let horizontal_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
        let speed = horizontal_velocity.length();
        let (idle_v, _cruise_v, idle_pitch, _cruise_pitch) = vehicle_audio_params(speed);

        sink.set_volume(Volume::Linear(idle_v * 0.45));
        sink.set_speed(idle_pitch);
    }

    // Update cruise loops
    for (entity, e, mut tf, mut sink) in cruise_emitters.iter_mut() {
        let Ok((driver, state, veh_tf)) = vehicles.get(e.vehicle) else {
            emitter_index.vehicle_cruise_by_emitter.remove(&entity);
            emitter_index.vehicle_cruise_targets.remove(&e.vehicle);
            commands.entity(entity).despawn();
            continue;
        };

        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                emitter_index.vehicle_cruise_by_emitter.remove(&entity);
                emitter_index.vehicle_cruise_targets.remove(&e.vehicle);
                commands.entity(entity).despawn();
                continue;
            }
        }

        let pos = veh_tf.translation;
        if pos.distance_squared(listener_pos) > despawn_distance_sq {
            emitter_index.vehicle_cruise_by_emitter.remove(&entity);
            emitter_index.vehicle_cruise_targets.remove(&e.vehicle);
            commands.entity(entity).despawn();
            continue;
        }

        tf.translation = pos;

        let horizontal_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
        let speed = horizontal_velocity.length();
        let (_idle_v, cruise_v, _idle_pitch, cruise_pitch) = vehicle_audio_params(speed);

        sink.set_volume(Volume::Linear(cruise_v * 0.7));
        sink.set_speed(cruise_pitch);
    }
}

/// Manage vehicle audio: spawn sounds when entering, despawn when exiting
pub fn update_vehicle_audio_state(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    mut vehicle_audio_state: ResMut<VehicleAudioState>,
    input_state: Res<InputState>,
    idle_sounds: Query<Entity, With<VehicleIdleSound>>,
    cruise_sounds: Query<Entity, With<VehicleCruiseSound>>,
) {
    // Don't do anything until assets are ready
    if !audio_state.assets_ready {
        return;
    }

    let Some(audio) = audio else { return };

    let in_vehicle = input_state.in_vehicle;
    let was_in_vehicle = vehicle_audio_state.was_in_vehicle;

    // Detect entering vehicle
    if in_vehicle && !was_in_vehicle {
        info!("Entering vehicle - spawning bike audio loops");

        // Spawn idle sound (starts playing)
        commands.spawn((
            VehicleIdleSound,
            AudioPlayer::new(audio.hover_idle.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.45)),
        ));

        // Spawn cruise sound (starts at zero volume, we'll crossfade in)
        commands.spawn((
            VehicleCruiseSound,
            AudioPlayer::new(audio.bike_cruise.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
        ));

        vehicle_audio_state.sounds_spawned = true;
    }

    // Detect exiting vehicle
    if !in_vehicle && was_in_vehicle {
        info!("Exiting vehicle - despawning bike audio");

        for entity in idle_sounds.iter() {
            commands.entity(entity).despawn();
        }
        for entity in cruise_sounds.iter() {
            commands.entity(entity).despawn();
        }

        vehicle_audio_state.sounds_spawned = false;
    }

    vehicle_audio_state.was_in_vehicle = in_vehicle;
}

/// Update vehicle audio: crossfade between idle/cruise and modulate pitch based on speed
pub fn update_vehicle_audio(
    input_state: Res<InputState>,
    vehicle_audio_state: Res<VehicleAudioState>,
    // Query for our local client to find which vehicle we're driving
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(&VehicleDriver, &VehicleState), With<Vehicle>>,
    // Use Without<T> to make these queries disjoint (avoids Bevy B0001 conflict)
    mut idle_sink: Query<&mut AudioSink, (With<VehicleIdleSound>, Without<VehicleCruiseSound>)>,
    mut cruise_sink: Query<&mut AudioSink, (With<VehicleCruiseSound>, Without<VehicleIdleSound>)>,
) {
    // Only process if we're in a vehicle with sounds spawned
    if !input_state.in_vehicle || !vehicle_audio_state.sounds_spawned {
        return;
    }

    // Get our peer ID
    let Some(our_peer_id) = client_query
        .iter()
        .next()
        .map(|id| crate::camera::peer_id_to_u64(id.0))
    else {
        return;
    };

    // Find the vehicle we're driving
    let Some((_, vehicle_state)) = vehicles
        .iter()
        .find(|(driver, _)| driver.driver_id == Some(our_peer_id))
    else {
        return;
    };

    // Calculate speed (horizontal only for audio purposes)
    let horizontal_velocity = Vec3::new(vehicle_state.velocity.x, 0.0, vehicle_state.velocity.z);
    let speed = horizontal_velocity.length();
    let (idle_volume, cruise_volume, idle_pitch, cruise_pitch) = vehicle_audio_params(speed);

    // Apply to idle sound
    if let Ok(mut sink) = idle_sink.single_mut() {
        sink.set_volume(Volume::Linear(idle_volume * 0.45)); // Base volume * crossfade
        sink.set_speed(idle_pitch);
    }

    // Apply to cruise sound
    if let Ok(mut sink) = cruise_sink.single_mut() {
        sink.set_volume(Volume::Linear(cruise_volume * 0.7)); // Base volume * crossfade
        sink.set_speed(cruise_pitch);
    }
}

/// Stop vehicle sounds when leaving gameplay
pub fn cleanup_vehicle_sounds(
    mut commands: Commands,
    mut vehicle_audio_state: ResMut<VehicleAudioState>,
    idle_sounds: Query<Entity, With<VehicleIdleSound>>,
    cruise_sounds: Query<Entity, With<VehicleCruiseSound>>,
) {
    for entity in idle_sounds.iter() {
        commands.entity(entity).despawn();
    }
    for entity in cruise_sounds.iter() {
        commands.entity(entity).despawn();
    }
    vehicle_audio_state.sounds_spawned = false;
    vehicle_audio_state.was_in_vehicle = false;
}
