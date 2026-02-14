use super::*;

/// Update and cleanup impact markers (fade via scale, shared materials)
pub fn update_impact_markers(
    mut commands: Commands,
    mut markers: Query<(Entity, &ImpactMarker, &mut Transform)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();

    // Enforce entity cap
    let count = markers.iter().len();
    if count > MAX_IMPACT_MARKERS {
        let mut by_age: Vec<(Entity, f32)> =
            markers.iter().map(|(e, m, _)| (e, m.spawn_time)).collect();
        by_age.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (entity, _) in by_age.iter().take(count - MAX_IMPACT_MARKERS) {
            commands.entity(*entity).despawn();
        }
    }

    for (entity, marker, mut transform) in markers.iter_mut() {
        let age = current_time - marker.spawn_time;

        if age > marker.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        // Expand over time, then shrink to zero for fade-out (shared material, can't mutate alpha)
        let t = age / marker.lifetime;
        let expand = 1.0 + t * 2.0; // Expand to 3x size
        let fade = (1.0 - t).powf(0.7); // Shrink towards end of life
        transform.scale = Vec3::splat(marker.base_scale * expand * fade);
    }
}
