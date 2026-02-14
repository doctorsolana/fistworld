//! Projectile cleanup/despawn systems.

use bevy::prelude::*;

use shared::components::{Bullet, BulletVelocity};
use shared::weapons::ballistics;

use crate::combat::bullet_sim::BulletPendingDespawn;

/// Clean up bullets that are out of bounds or expired.
pub fn cleanup_bullets(
    mut commands: Commands,
    bullets: Query<(
        Entity,
        &Bullet,
        &BulletVelocity,
        &Transform,
        Option<&BulletPendingDespawn>,
    )>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();

    for (entity, bullet, velocity, transform, pending) in bullets.iter() {
        if let Some(pending) = pending {
            if current_time >= pending.despawn_at {
                commands.entity(entity).despawn();
            }
            continue;
        }

        if ballistics::should_despawn_bullet(
            velocity.0,
            bullet.spawn_position,
            transform.translation,
            bullet.spawn_time,
            current_time,
        ) {
            commands.entity(entity).despawn();
        }

        if transform.translation.y < -50.0 {
            commands.entity(entity).despawn();
        }
    }
}
