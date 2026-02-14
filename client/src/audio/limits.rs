//! limits systems.

use super::*;

/// Enforce global audio limits by despawning lowest priority sounds when over limit
pub fn apply_audio_limits(
    mut commands: Commands,
    audio_manager: Res<AudioManager>,
    camera: Query<&Transform, With<Camera3d>>,
    managed_audio: Query<(Entity, &ManagedAudioTag, &Transform)>,
) {
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;

    // Fast path: avoid allocation/sort when we're under the cap.
    let current_count = managed_audio.iter().count();
    if current_count <= audio_manager.max_total {
        return;
    }

    // Collect only when we know we overflow.
    let mut audio_list: Vec<(Entity, AudioPriority, f32, f32)> = managed_audio
        .iter()
        .map(|(entity, tag, tf)| {
            let dist_sq = tf.translation.distance_squared(listener_pos);
            (entity, tag.priority, dist_sq, tag.spawn_time)
        })
        .collect();

    let excess = current_count - audio_manager.max_total;

    // Sort by priority (ascending), then distance (descending), then age (oldest first)
    // This puts lowest-priority, farthest, oldest sounds at the front for removal
    audio_list.sort_by(|a, b| {
        match a.1.cmp(&b.1) {
            std::cmp::Ordering::Equal => {
                // Same priority: farther sounds should be removed first
                match b.2.partial_cmp(&a.2) {
                    Some(std::cmp::Ordering::Equal) | None => {
                        // Same distance: older sounds removed first
                        a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    Some(ord) => ord,
                }
            }
            ord => ord,
        }
    });

    // Despawn the excess lowest-priority sounds
    for (entity, priority, _dist, _time) in audio_list.into_iter().take(excess) {
        commands.entity(entity).despawn();
        trace!(
            "Audio limit: despawned {:?} (priority {:?})",
            entity,
            priority
        );
    }
}
