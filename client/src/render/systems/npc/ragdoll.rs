//! Client-side authoritative ragdoll playback.

use super::*;
use lightyear::prelude::{Connected, MessageReceiver};
use shared::protocol::{
    unpack_quat_i16, NpcRagdollPoseBatch, NpcRagdollPoseSample, NpcRagdollStarted, RagdollBodyId,
};

fn decode_sample(sample: &NpcRagdollPoseSample, now: f32) -> NpcRagdollPoseFrame {
    NpcRagdollPoseFrame {
        seq: sample.seq,
        received_at: now,
        root_position: sample.root_position,
        root_rotation: unpack_quat_i16(sample.root_rotation),
        bodies: sample
            .bodies
            .iter()
            .map(|pose| NpcRagdollBodyPoseFrame {
                body: pose.body,
                position: pose.position,
                rotation: unpack_quat_i16(pose.rotation),
            })
            .collect(),
    }
}

fn expected_ragdoll_bodies() -> [RagdollBodyId; 12] {
    [
        RagdollBodyId::Pelvis,
        RagdollBodyId::SpineLower,
        RagdollBodyId::SpineUpper,
        RagdollBodyId::Head,
        RagdollBodyId::UpperArmL,
        RagdollBodyId::ForearmL,
        RagdollBodyId::UpperArmR,
        RagdollBodyId::ForearmR,
        RagdollBodyId::ThighL,
        RagdollBodyId::CalfL,
        RagdollBodyId::ThighR,
        RagdollBodyId::CalfR,
    ]
}

fn is_newer_seq(new_seq: u32, old_seq: u32) -> bool {
    new_seq != old_seq && new_seq.wrapping_sub(old_seq) < (u32::MAX / 2)
}

fn body_pose_for(
    frame: &NpcRagdollPoseFrame,
    body: RagdollBodyId,
) -> Option<NpcRagdollBodyPoseFrame> {
    frame.bodies.iter().copied().find(|pose| pose.body == body)
}

fn ragdoll_parent(body: RagdollBodyId) -> Option<RagdollBodyId> {
    match body {
        RagdollBodyId::Pelvis => None,
        RagdollBodyId::SpineLower => Some(RagdollBodyId::Pelvis),
        RagdollBodyId::SpineUpper => Some(RagdollBodyId::SpineLower),
        RagdollBodyId::Head => Some(RagdollBodyId::SpineUpper),
        RagdollBodyId::UpperArmL | RagdollBodyId::UpperArmR => Some(RagdollBodyId::SpineUpper),
        RagdollBodyId::ForearmL => Some(RagdollBodyId::UpperArmL),
        RagdollBodyId::ForearmR => Some(RagdollBodyId::UpperArmR),
        RagdollBodyId::ThighL | RagdollBodyId::ThighR => Some(RagdollBodyId::Pelvis),
        RagdollBodyId::CalfL => Some(RagdollBodyId::ThighL),
        RagdollBodyId::CalfR => Some(RagdollBodyId::ThighR),
    }
}

fn ragdoll_local_rotation(
    body: RagdollBodyId,
    world_by_body: &std::collections::HashMap<RagdollBodyId, Quat>,
    root_rotation: Quat,
) -> Option<Quat> {
    let world = world_by_body.get(&body).copied()?;
    let parent_world = ragdoll_parent(body)
        .and_then(|parent| world_by_body.get(&parent).copied())
        .unwrap_or(root_rotation);
    Some((parent_world.inverse() * world).normalize())
}

pub fn receive_ragdoll_started(
    mut commands: Commands,
    time: Res<Time>,
    mut client_query: Query<
        &mut MessageReceiver<NpcRagdollStarted>,
        (With<crate::GameClient>, With<Connected>),
    >,
    npcs: Query<(Entity, &Npc)>,
    anim_roots: Query<(&NpcRigOwner, &NpcBoneMap, Option<&NpcBindPose>), With<NpcAnimationRoot>>,
    mut anim_players: Query<
        (
            &NpcRigOwner,
            &mut AnimationPlayer,
            Option<&mut NpcAnimState>,
        ),
        With<NpcAnimationRoot>,
    >,
    bone_transforms: Query<&Transform, (Without<Npc>, Without<NpcAnimationRoot>)>,
) {
    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };
    let now = time.elapsed_secs();

    let npc_by_id: std::collections::HashMap<u64, Entity> =
        npcs.iter().map(|(entity, npc)| (npc.id, entity)).collect();
    let rig_by_owner: std::collections::HashMap<Entity, (&NpcBoneMap, Option<&NpcBindPose>)> =
        anim_roots
            .iter()
            .map(|(owner, map, bind_pose)| (owner.0, (map, bind_pose)))
            .collect();

    for started in receiver.receive() {
        let Some(&npc_entity) = npc_by_id.get(&started.npc_id) else {
            continue;
        };
        let root_rotation = unpack_quat_i16(started.root_rotation);
        let world_by_body: std::collections::HashMap<RagdollBodyId, Quat> = started
            .bodies
            .iter()
            .map(|pose| (pose.body, unpack_quat_i16(pose.rotation)))
            .collect();

        let mut local_rotation_corrections = std::collections::HashMap::new();
        if let Some((bone_map, bind_pose)) = rig_by_owner.get(&npc_entity) {
            for (body, bone_entity) in &bone_map.bones {
                let Some(ragdoll_local) =
                    ragdoll_local_rotation(*body, &world_by_body, root_rotation)
                else {
                    continue;
                };
                let bind_local = bind_pose
                    .and_then(|pose| pose.body_local_rotations.get(body).copied())
                    .or_else(|| bone_transforms.get(*bone_entity).ok().map(|tf| tf.rotation));
                let Some(bind_local) = bind_local else {
                    continue;
                };
                // Match ragdoll local joint frame to the existing Mixamo local bone frame
                // so incoming world-space rigid-body orientations don't collapse the rig.
                local_rotation_corrections
                    .insert(*body, (bind_local * ragdoll_local.inverse()).normalize());
            }
        }

        let frame = NpcRagdollPoseFrame {
            seq: 0,
            received_at: now,
            root_position: started.root_position,
            root_rotation: unpack_quat_i16(started.root_rotation),
            bodies: started
                .bodies
                .iter()
                .map(|pose| NpcRagdollBodyPoseFrame {
                    body: pose.body,
                    position: pose.position,
                    rotation: unpack_quat_i16(pose.rotation),
                })
                .collect(),
        };

        if let Some((bone_map, _bind_pose)) = rig_by_owner.get(&npc_entity) {
            let expected = expected_ragdoll_bodies();
            let packet_bodies = started
                .bodies
                .iter()
                .map(|p| p.body)
                .collect::<std::collections::HashSet<_>>();
            let missing_in_map = expected
                .iter()
                .copied()
                .filter(|body| !bone_map.bones.contains_key(body))
                .collect::<Vec<_>>();
            let missing_in_packet = expected
                .iter()
                .copied()
                .filter(|body| !packet_bodies.contains(body))
                .collect::<Vec<_>>();
            if !missing_in_map.is_empty() || !missing_in_packet.is_empty() {
                warn!(
                    "Ragdoll body coverage issue for npc_entity={:?}: mapped={}/12 packet={}/12 missing_map={:?} missing_packet={:?}",
                    npc_entity,
                    bone_map.bones.len().min(12),
                    packet_bodies.len().min(12),
                    missing_in_map,
                    missing_in_packet
                );
            } else {
                info!(
                    "Ragdoll body coverage OK for npc_entity={:?}: mapped=12 packet=12",
                    npc_entity
                );
            }
        } else {
            warn!(
                "Ragdoll started for npc_entity={:?} but no animation rig map found yet",
                npc_entity
            );
        }

        commands.entity(npc_entity).insert((
            NpcRagdollActive,
            NpcRagdollNetState {
                prev: None,
                curr: Some(frame),
                local_rotation_corrections,
            },
        ));

        for (owner, mut player, state) in anim_players.iter_mut() {
            if owner.0 != npc_entity {
                continue;
            }
            player.stop_all();
            if let Some(mut state) = state {
                state.dead = true;
                state.current_anim = NpcMovementAnim::Idle;
                state.target_anim = NpcMovementAnim::Idle;
                state.blend_progress = 1.0;
                state.transition_lock_timer = 0.0;
            }
            break;
        }
    }
}

pub fn receive_ragdoll_pose_batch(
    mut commands: Commands,
    time: Res<Time>,
    mut client_query: Query<
        &mut MessageReceiver<NpcRagdollPoseBatch>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut npcs: Query<(
        Entity,
        &Npc,
        Option<&NpcRagdollActive>,
        &mut NpcRagdollNetState,
    )>,
) {
    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };
    let now = time.elapsed_secs();

    let mut latest_by_npc: std::collections::HashMap<u64, NpcRagdollPoseFrame> =
        std::collections::HashMap::new();
    for batch in receiver.receive() {
        for sample in &batch.samples {
            let decoded = decode_sample(sample, now);
            match latest_by_npc.get(&sample.npc_id) {
                Some(current) if !is_newer_seq(decoded.seq, current.seq) => {}
                _ => {
                    latest_by_npc.insert(sample.npc_id, decoded);
                }
            }
        }
    }

    if latest_by_npc.is_empty() {
        return;
    }

    for (entity, npc, ragdoll_active, mut state) in npcs.iter_mut() {
        let Some(new_frame) = latest_by_npc.remove(&npc.id) else {
            continue;
        };

        let accept = state
            .curr
            .as_ref()
            .map(|curr| is_newer_seq(new_frame.seq, curr.seq))
            .unwrap_or(true);
        if !accept {
            continue;
        }

        state.prev = state.curr.take();
        state.curr = Some(new_frame);
        if ragdoll_active.is_none() {
            commands.entity(entity).insert(NpcRagdollActive);
        }
    }
}

pub fn apply_ragdoll_pose(
    time: Res<Time>,
    mut npcs: Query<
        (Entity, &mut Transform, &NpcRagdollNetState),
        (With<Npc>, With<NpcRagdollActive>),
    >,
    anim_roots: Query<(&NpcRigOwner, &NpcBoneMap, Option<&NpcBindPose>), With<NpcAnimationRoot>>,
    mut bone_transforms: Query<&mut Transform, (Without<Npc>, Without<NpcAnimationRoot>)>,
) {
    let now = time.elapsed_secs();
    let rig_by_owner: std::collections::HashMap<Entity, (&NpcBoneMap, Option<&NpcBindPose>)> =
        anim_roots
            .iter()
            .map(|(owner, map, bind_pose)| (owner.0, (map, bind_pose)))
            .collect();

    const MIN_BONE_SAMPLE_COUNT: usize = 4;

    for (npc_entity, mut root_transform, net_state) in npcs.iter_mut() {
        let Some(curr) = net_state.curr.as_ref() else {
            continue;
        };

        let (root_position, root_rotation, body_poses) = if let Some(prev) = net_state.prev.as_ref()
        {
            let dt = (curr.received_at - prev.received_at).max(1.0 / 240.0);
            let render_delay = 0.050;
            let t = ((now - render_delay - prev.received_at) / dt).clamp(0.0, 1.0);
            let mut blended = Vec::with_capacity(curr.bodies.len());
            for curr_pose in &curr.bodies {
                let pose = if let Some(prev_pose) = body_pose_for(prev, curr_pose.body) {
                    NpcRagdollBodyPoseFrame {
                        body: curr_pose.body,
                        position: prev_pose.position.lerp(curr_pose.position, t),
                        rotation: prev_pose.rotation.slerp(curr_pose.rotation, t),
                    }
                } else {
                    *curr_pose
                };
                blended.push(pose);
            }
            (
                prev.root_position.lerp(curr.root_position, t),
                prev.root_rotation.slerp(curr.root_rotation, t),
                blended,
            )
        } else {
            (curr.root_position, curr.root_rotation, curr.bodies.clone())
        };

        root_transform.translation = root_position;
        root_transform.rotation = root_rotation;

        let Some((bone_map, bind_pose)) = rig_by_owner.get(&npc_entity) else {
            continue;
        };
        // Reduced-body ragdoll v1 can provide root-only or sparse samples.
        // Skip direct bone driving unless we have a fuller body sample set;
        // sparse rotation application can collapse Mixamo rigs into a "ball".
        if body_poses.len() < MIN_BONE_SAMPLE_COUNT {
            continue;
        }
        if let Some(bind_pose) = bind_pose {
            for (bone_entity, bind_local_rot) in &bind_pose.all_bones {
                if let Ok(mut bone_tf) = bone_transforms.get_mut(*bone_entity) {
                    bone_tf.rotation = *bind_local_rot;
                }
            }
        }
        let world_by_body: std::collections::HashMap<RagdollBodyId, Quat> = body_poses
            .iter()
            .map(|pose| (pose.body, pose.rotation))
            .collect();
        for body in world_by_body.keys().copied() {
            let Some(&bone_entity) = bone_map.bones.get(&body) else {
                continue;
            };
            let Ok(mut bone_transform) = bone_transforms.get_mut(bone_entity) else {
                continue;
            };
            let Some(ragdoll_local) = ragdoll_local_rotation(body, &world_by_body, root_rotation)
            else {
                continue;
            };
            let correction = net_state
                .local_rotation_corrections
                .get(&body)
                .copied()
                .unwrap_or(Quat::IDENTITY);
            let target_local = (correction * ragdoll_local).normalize();
            bone_transform.rotation = bone_transform.rotation.slerp(target_local, 0.65);
        }
    }
}
