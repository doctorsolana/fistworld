//! NPC debug visualization systems.

use super::*;

pub fn update_npc_hitbox_debug_gizmos(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    npcs: Query<(&Transform, &Health), With<Npc>>,
) {
    if !debug_mode.0 {
        return;
    }

    for (transform, health) in npcs.iter() {
        let center = transform.translation;

        let head_center = npc_head_center(center);
        let (a, b) = npc_capsule_endpoints(center);

        let alive = !health.is_dead();

        let body_color = if alive {
            Color::srgba(1.0, 0.85, 0.2, 0.9)
        } else {
            Color::srgba(0.6, 0.6, 0.6, 0.7)
        };
        let head_color = if alive {
            Color::srgba(1.0, 0.2, 0.2, 0.95)
        } else {
            Color::srgba(0.5, 0.2, 0.2, 0.7)
        };

        // Body capsule (approx): spheres at endpoints + line between.
        gizmos.sphere(Isometry3d::from_translation(a), NPC_RADIUS, body_color);
        gizmos.sphere(Isometry3d::from_translation(b), NPC_RADIUS, body_color);
        gizmos.line(a, b, body_color);

        // Head sphere.
        gizmos.sphere(
            Isometry3d::from_translation(head_center),
            NPC_HEAD_RADIUS,
            head_color,
        );
    }
}
