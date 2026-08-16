//! Shared tactical actor helpers used across village domains.

use super::*;

pub(super) fn stable_name_hash(name: &str) -> u32 {
    name.bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(byte as u32)
    })
}

pub(crate) fn ensure_move_target(
    commands: &mut Commands,
    entity: Entity,
    current: Option<&MoveTarget>,
    expected: Vec3,
) {
    if current.is_none_or(|target| ground_distance(target.0, expected) > 0.05) {
        commands.entity(entity).insert(MoveTarget(expected));
    }
}
