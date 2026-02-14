//! Projectile simulation systems.

use bevy::prelude::*;

use shared::components::{Bullet, BulletPrevPosition, BulletVelocity, PlayerPosition};
use shared::protocol::FIXED_TIMESTEP_HZ;
use shared::weapons::ballistics;

/// Server-only marker used to delay bullet despawn by a few frames.
///
/// This prevents clients from receiving a despawn for a bullet entity that was
/// spawned and destroyed within the same replication tick.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct BulletPendingDespawn {
    pub(crate) despawn_at: f32,
}

/// Simulate bullet physics.
pub fn update_bullets(
    mut bullets: Query<
        (
            &Bullet,
            &mut BulletVelocity,
            &mut BulletPrevPosition,
            &mut Transform,
            &mut PlayerPosition,
        ),
        Without<BulletPendingDespawn>,
    >,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (_bullet, mut velocity, mut prev_pos, mut transform, mut position) in bullets.iter_mut() {
        prev_pos.0 = transform.translation;

        let (new_pos, new_vel) =
            ballistics::step_bullet_physics(transform.translation, velocity.0, dt);

        transform.translation = new_pos;
        position.0 = new_pos;
        velocity.0 = new_vel;
    }
}
