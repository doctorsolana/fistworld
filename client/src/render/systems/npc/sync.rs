//! sync systems.

use super::*;

/// Sync NPC transforms from replicated components.
pub fn sync_npc_transforms(
    time: Res<Time>,
    mut npcs: Query<(&NpcPosition, &NpcRotation, &mut Transform), With<Npc>>,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 18.0;
    let rot_rate: f32 = 22.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    for (pos, rot, mut transform) in npcs.iter_mut() {
        transform.translation = transform.translation.lerp(pos.0, t_pos);
        let target_rot = Quat::from_rotation_y(rot.0);
        transform.rotation = transform.rotation.slerp(target_rot, t_rot);
    }
}
