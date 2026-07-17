//! NPC debug visualization systems.

use super::*;

fn ragdoll_parent(
    body: shared::protocol::RagdollBodyId,
) -> Option<shared::protocol::RagdollBodyId> {
    use shared::protocol::RagdollBodyId as B;
    match body {
        B::Pelvis => None,
        B::SpineLower => Some(B::Pelvis),
        B::SpineUpper => Some(B::SpineLower),
        B::Head => Some(B::SpineUpper),
        B::UpperArmL | B::UpperArmR => Some(B::SpineUpper),
        B::ForearmL => Some(B::UpperArmL),
        B::ForearmR => Some(B::UpperArmR),
        B::HandL => Some(B::ForearmL),
        B::HandR => Some(B::ForearmR),
        B::ThighL | B::ThighR => Some(B::Pelvis),
        B::CalfL => Some(B::ThighL),
        B::CalfR => Some(B::ThighR),
        B::FootL => Some(B::CalfL),
        B::FootR => Some(B::CalfR),
    }
}

pub fn update_npc_hitbox_debug_gizmos(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    npcs: Query<(&Npc, &Transform, &Health)>,
) {
    if !debug_mode.0 {
        return;
    }

    for (npc, transform, health) in npcs.iter() {
        let center = transform.translation;
        let alive = !health.is_dead();

        if alive
            && matches!(
                npc.archetype,
                shared::components::NpcArchetype::Dummy
                    | shared::components::NpcArchetype::CombatDummy
            )
        {
            for def in HUMANOID_RAGDOLL_BODIES {
                let part = humanoid_body_part(def.id);
                let color = match part.hit_zone() {
                    shared::weapons::damage::HitZone::Head => Color::srgba(1.0, 0.18, 0.12, 0.95),
                    shared::weapons::damage::HitZone::Chest => Color::srgba(0.15, 0.65, 1.0, 0.9),
                    shared::weapons::damage::HitZone::Stomach => Color::srgba(0.3, 1.0, 0.35, 0.9),
                    shared::weapons::damage::HitZone::Arms => Color::srgba(1.0, 0.72, 0.15, 0.9),
                    shared::weapons::damage::HitZone::Legs => Color::srgba(0.78, 0.4, 1.0, 0.9),
                };
                let body_center = center + transform.rotation * def.local_offset;
                let body_rotation =
                    transform.rotation * Quat::from_rotation_arc(Vec3::Y, ragdoll_body_axis(&def));
                let isometry = Isometry3d::new(body_center, body_rotation);
                match humanoid_body_shape(def.id) {
                    HumanoidBodyShape::Sphere { radius } => {
                        gizmos.sphere(isometry, radius, color);
                    }
                    HumanoidBodyShape::Capsule {
                        half_segment,
                        radius,
                    } => {
                        gizmos.primitive_3d(
                            &Capsule3d::new(radius, half_segment * 2.0),
                            isometry,
                            color,
                        );
                    }
                    HumanoidBodyShape::Cuboid { half_extents } => {
                        gizmos.primitive_3d(
                            &Cuboid::from_size(half_extents * 2.0),
                            isometry,
                            color,
                        );
                    }
                }
            }
            continue;
        }

        let head_center = npc_head_center(center);
        let (a, b) = npc_capsule_endpoints(center);

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

/// Draw mapped animation-bone skeleton and authoritative ragdoll body links.
/// This runs under the same F4 debug toggle to help diagnose bad bone mapping
/// vs bad server ragdoll constraints.
pub fn update_npc_ragdoll_debug_gizmos(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    npcs: Query<
        (
            Entity,
            Option<&NpcRagdollActive>,
            Option<&NpcRagdollNetState>,
        ),
        With<Npc>,
    >,
    anim_roots: Query<(&NpcRigOwner, &NpcBoneMap), With<NpcAnimationRoot>>,
    bone_globals: Query<&GlobalTransform>,
) {
    use shared::protocol::RagdollBodyId as B;

    if !debug_mode.0 {
        return;
    }

    let rig_by_owner = anim_roots
        .iter()
        .map(|(owner, map)| (owner.0, map))
        .collect::<std::collections::HashMap<_, _>>();

    for (npc_entity, ragdoll_active, ragdoll_net) in npcs.iter() {
        let Some(bone_map) = rig_by_owner.get(&npc_entity).copied() else {
            continue;
        };

        // 1) Bone skeleton from mapped rig entities (cyan).
        let mut bone_pos_by_body = std::collections::HashMap::new();
        for (body, bone_entity) in &bone_map.bones {
            let Ok(global) = bone_globals.get(*bone_entity) else {
                continue;
            };
            let pos = global.translation();
            bone_pos_by_body.insert(*body, pos);
            gizmos.sphere(
                Isometry3d::from_translation(pos),
                0.035,
                Color::srgba(0.1, 0.9, 1.0, 0.95),
            );
        }
        for (body, pos) in bone_pos_by_body.iter() {
            if let Some(parent) = ragdoll_parent(*body) {
                if let Some(parent_pos) = bone_pos_by_body.get(&parent) {
                    gizmos.line(*parent_pos, *pos, Color::srgba(0.1, 0.9, 1.0, 0.75));
                }
            }
        }

        // 2) Authoritative ragdoll sample skeleton (orange/red), if active.
        let Some(_active) = ragdoll_active else {
            continue;
        };
        let Some(net_state) = ragdoll_net else {
            continue;
        };
        let Some(frame) = net_state.curr.as_ref() else {
            continue;
        };

        let mut body_pos_by_body = std::collections::HashMap::new();
        for pose in &frame.bodies {
            body_pos_by_body.insert(pose.body, pose.position);
            gizmos.sphere(
                Isometry3d::from_translation(pose.position),
                0.04,
                Color::srgba(1.0, 0.55, 0.1, 0.95),
            );
        }
        for (body, pos) in body_pos_by_body.iter() {
            if let Some(parent) = ragdoll_parent(*body) {
                if let Some(parent_pos) = body_pos_by_body.get(&parent) {
                    gizmos.line(*parent_pos, *pos, Color::srgba(1.0, 0.35, 0.15, 0.85));
                }
            }
        }

        // 3) Drift between authoritative ragdoll bodies and mapped animated bones (yellow).
        for body in [
            B::Pelvis,
            B::SpineLower,
            B::SpineUpper,
            B::Head,
            B::UpperArmL,
            B::ForearmL,
            B::UpperArmR,
            B::ForearmR,
            B::ThighL,
            B::CalfL,
            B::ThighR,
            B::CalfR,
        ] {
            let Some(body_pos) = body_pos_by_body.get(&body).copied() else {
                continue;
            };
            let Some(bone_pos) = bone_pos_by_body.get(&body).copied() else {
                continue;
            };
            gizmos.line(body_pos, bone_pos, Color::srgba(1.0, 0.95, 0.2, 0.75));
        }
    }
}
