//! debug systems.

use super::*;

/// Debug: Draw bullet trajectories
/// Uses thick lines to be visible with HDR/bloom
pub fn update_trajectory_debug_gizmos(
    bullets: Query<(&Bullet, &Transform, &BulletTrail)>,
    mut gizmos: Gizmos,
    debug_mode: Res<DebugGizmoMode>,
    debug_trails: Res<DebugBulletTrails>,
    time: Res<Time>,
) {
    if !debug_mode.0 {
        return;
    }

    let now = time.elapsed_secs();

    // Draw live bullet trails (replicated) - bright green
    for (bullet, transform, trail) in bullets.iter() {
        // Line from spawn to current position - thick bright line
        gizmos.line(
            bullet.spawn_position,
            transform.translation,
            Color::srgb(0.2, 1.0, 0.2), // Bright green
        );

        // Draw trail points
        for window in trail.positions.windows(2) {
            gizmos.line(window[0], window[1], Color::srgba(0.2, 1.0, 0.2, 0.7));
        }

        // Draw sphere at spawn position for visibility
        gizmos.sphere(
            Isometry3d::from_translation(bullet.spawn_position),
            0.15,
            Color::srgb(0.0, 1.0, 0.0),
        );
    }

    // Draw persistent debug trails with fading - bright red/orange
    for (trail, spawn_time, _base_color) in debug_trails.trails.iter() {
        let age = now - spawn_time;
        let alpha = (1.0 - age / 10.0).clamp(0.0, 1.0);

        // Use bright red/orange for better visibility with HDR
        let color = Color::srgba(1.0, 0.3, 0.0, alpha);

        for window in trail.windows(2) {
            gizmos.line(window[0], window[1], color);
        }

        // Draw sphere at start of trail
        if let Some(first) = trail.first() {
            gizmos.sphere(
                Isometry3d::from_translation(*first),
                0.1,
                Color::srgba(1.0, 0.0, 0.0, alpha),
            );
        }
    }
}
