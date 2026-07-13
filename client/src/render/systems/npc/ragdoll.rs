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

/// Compute the per-body rotation offsets `O = B0⁻¹ · W0(bone)` so per frame
/// the desired bone world rotation is exactly `B · O`.
///
/// CRITICAL: `root_rotation_ref` must be derived from SERVER-side data (the
/// started message's root frame), never from the client's smoothed NPC
/// transform — the client lags the server by the net-smoothing error at the
/// death instant, and baking that mismatch into the offsets warps every limb
/// in a different direction as the bodies rotate.
fn compute_body_bone_offsets(
    rig: &NpcRagdollRig,
    root_rotation_ref: Quat,
    start_body_rotations: &std::collections::HashMap<RagdollBodyId, Quat>,
) -> std::collections::HashMap<RagdollBodyId, Quat> {
    let mut rot_offsets = std::collections::HashMap::new();
    for bone in &rig.bones {
        let Some(body) = bone.body else { continue };
        let Some(&body_start_rot) = start_body_rotations.get(&body) else {
            continue;
        };
        let bone_bind_world_rot = (root_rotation_ref * bone.bind_rel.rotation).normalize();
        rot_offsets.insert(
            body,
            (body_start_rot.inverse() * bone_bind_world_rot).normalize(),
        );
    }
    rot_offsets
}

pub fn receive_ragdoll_started(
    mut commands: Commands,
    time: Res<Time>,
    mut client_query: Query<
        &mut MessageReceiver<NpcRagdollStarted>,
        (With<crate::GameClient>, With<Connected>),
    >,
    npcs: Query<(Entity, &Npc, &Transform)>,
    anim_roots: Query<(&NpcRigOwner, &NpcBoneMap, Option<&NpcRagdollRig>), With<NpcAnimationRoot>>,
    mut anim_players: Query<
        (
            &NpcRigOwner,
            &mut AnimationPlayer,
            Option<&mut NpcAnimState>,
        ),
        With<NpcAnimationRoot>,
    >,
) {
    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };
    let now = time.elapsed_secs();

    let npc_by_id: std::collections::HashMap<u64, (Entity, Quat, Vec3)> = npcs
        .iter()
        .map(|(entity, npc, transform)| {
            (npc.id, (entity, transform.rotation, transform.translation))
        })
        .collect();
    let rig_by_owner: std::collections::HashMap<Entity, (&NpcBoneMap, Option<&NpcRagdollRig>)> =
        anim_roots
            .iter()
            .map(|(owner, map, rig)| (owner.0, (map, rig)))
            .collect();

    for started in receiver.receive() {
        let Some(&(npc_entity, _root_rotation_now, _root_translation_now)) =
            npc_by_id.get(&started.npc_id)
        else {
            continue;
        };
        let start_body_rotations: std::collections::HashMap<RagdollBodyId, Quat> = started
            .bodies
            .iter()
            .map(|pose| (pose.body, unpack_quat_i16(pose.rotation)))
            .collect();

        // Reference root frame from SERVER data only: the started message's
        // root rotation IS the NPC-root yaw (the server ragdoll frame has no
        // yaw offset — a PI offset here mirrors left/right body assignment).
        let root_rotation_ref = unpack_quat_i16(started.root_rotation).normalize();

        let body_bone_offsets = rig_by_owner
            .get(&npc_entity)
            .and_then(|(_, rig)| rig.as_ref().copied())
            .map(|rig| compute_body_bone_offsets(rig, root_rotation_ref, &start_body_rotations))
            .unwrap_or_default();

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

        if let Some((bone_map, _rig)) = rig_by_owner.get(&npc_entity) {
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
        } else if started.archetype != shared::components::NpcArchetype::Dummy {
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
                body_bone_offsets,
                start_body_rotations,
                root_fix_rotation: Some(root_rotation_ref),
                started_at: now,
                pose_reset_done: false,
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

/// Seconds over which the mesh cross-fades from its death-frame animation
/// pose into the streamed ragdoll pose (hides the one-frame snap between the
/// animated pose and the server's canonical spawn layout).
const RAGDOLL_BLEND_IN_SECS: f32 = 0.15;
/// Exponential smoothing rate for per-bone rotation targets (higher = snappier).
const RAGDOLL_BONE_SMOOTHING_RATE: f32 = 22.0;

pub fn apply_ragdoll_pose(
    time: Res<Time>,
    mut npcs: Query<
        (Entity, &mut Transform, &mut NpcRagdollNetState),
        (With<Npc>, With<NpcRagdollActive>),
    >,
    anim_roots: Query<(&NpcRigOwner, Option<&NpcRagdollRig>), With<NpcAnimationRoot>>,
    children_q: Query<&Children>,
    dummy_bodies: Query<&NpcDummyBody>,
    mut bone_transforms: Query<&mut Transform, (Without<Npc>, Without<NpcAnimationRoot>)>,
) {
    let now = time.elapsed_secs();
    let frame_dt = time.delta_secs().max(1.0 / 240.0);
    let rig_by_owner: std::collections::HashMap<Entity, &NpcRagdollRig> = anim_roots
        .iter()
        .filter_map(|(owner, rig)| rig.map(|rig| (owner.0, rig)))
        .collect();

    for (npc_entity, mut root_transform, mut net_state) in npcs.iter_mut() {
        let Some(curr) = net_state.curr.as_ref() else {
            continue;
        };

        // Interpolate between the two latest network samples (50ms render delay).
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

        let Some(rig) = rig_by_owner.get(&npc_entity).copied() else {
            // No skinned rig: track the streamed root directly. For reference
            // Dummies, drive each primitive body part 1:1 from its streamed
            // world pose — no mapping of any kind, pure ground truth.
            root_transform.translation = root_position;
            root_transform.rotation = root_rotation;
            let root_rot_inv = root_rotation.inverse();
            if let Ok(children) = children_q.get(npc_entity) {
                for child in children.iter() {
                    let Ok(dummy) = dummy_bodies.get(child) else {
                        continue;
                    };
                    let Some(pose) = body_poses.iter().find(|pose| pose.body == dummy.body)
                    else {
                        continue;
                    };
                    if let Ok(mut part_transform) = bone_transforms.get_mut(child) {
                        part_transform.translation =
                            root_rot_inv * (pose.position - root_position);
                        part_transform.rotation = (root_rot_inv * pose.rotation).normalize();
                    }
                }
            }
            continue;
        };

        // Offsets may not have been computable at activation (rig spawned
        // later); derive them lazily from the stored start snapshot. The
        // reference rotation was captured from the server's started message.
        if net_state.body_bone_offsets.is_empty() && !net_state.start_body_rotations.is_empty() {
            let root_rot_ref = net_state
                .root_fix_rotation
                .unwrap_or(root_transform.rotation);
            net_state.body_bone_offsets =
                compute_body_bone_offsets(rig, root_rot_ref, &net_state.start_body_rotations);
            net_state.root_fix_rotation = Some(root_rot_ref);
        }

        // Anchor: freeze the root at its death yaw and translate it so the
        // hips bone lands exactly on the streamed pelvis body. All corpse
        // orientation is expressed through the bones, which sidesteps every
        // model-root/armature frame convention.
        let root_fix = net_state
            .root_fix_rotation
            .unwrap_or(root_transform.rotation);
        root_transform.rotation = root_fix;
        root_transform.translation = root_position - root_fix * rig.hips_offset_from_root;

        if body_poses.is_empty() {
            continue;
        }
        let stream_rot_by_body: std::collections::HashMap<RagdollBodyId, Quat> = body_poses
            .iter()
            .map(|pose| (pose.body, pose.rotation))
            .collect();

        // One-shot reference reset at ragdoll start: unmapped bones (e.g. the
        // shoulders between spine and arms) otherwise keep their death-frame
        // ANIMATION pose, which displaces every child bone's attach point —
        // that was the "all corpses curl into the same pose" bug. Bone
        // translations stay at bind from here on (mesh proportions preserved).
        if !net_state.pose_reset_done {
            for bone in rig.bones.iter() {
                if let Ok(mut bone_transform) = bone_transforms.get_mut(bone.entity) {
                    *bone_transform = bone.bind_local;
                }
            }
            net_state.pose_reset_done = true;
        }

        // Blend-in weight (death-pose -> ragdoll) plus per-frame smoothing.
        let blend_in = ((now - net_state.started_at) / RAGDOLL_BLEND_IN_SECS).clamp(0.15, 1.0);
        let alpha = (1.0 - (-frame_dt * RAGDOLL_BONE_SMOOTHING_RATE).exp()) * blend_in;

        // Top-down solve: walk bones parent-before-child accumulating world
        // rotations from the *written* values, so each local conversion is
        // exact for the actual parent pose (uniform armature scale commutes
        // with pure rotations).
        let prefix_rot = rig.prefix.rotation;
        let mut world_rots: Vec<Quat> = Vec::with_capacity(rig.bones.len());
        for bone in rig.bones.iter() {
            let parent_world = match bone.parent {
                Some(parent_index) => world_rots[parent_index],
                None => root_fix * prefix_rot,
            };

            let desired_local = bone.body.and_then(|body| {
                let stream_rot = stream_rot_by_body.get(&body)?;
                let rot_offset = net_state.body_bone_offsets.get(&body)?;
                let desired_world_rot = (*stream_rot * *rot_offset).normalize();
                Some((parent_world.inverse() * desired_world_rot).normalize())
            });

            if let Ok(mut bone_transform) = bone_transforms.get_mut(bone.entity) {
                if let Some(target_local_rot) = desired_local {
                    bone_transform.rotation = bone_transform
                        .rotation
                        .slerp(target_local_rot, alpha)
                        .normalize();
                }
                world_rots.push(parent_world * bone_transform.rotation);
            } else {
                world_rots.push(parent_world * bone.bind_local.rotation);
            }
        }
    }
}
