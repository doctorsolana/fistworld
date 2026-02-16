//! spawn systems.

use super::*;
use shared::protocol::RagdollBodyId;
use std::collections::HashMap;

fn ragdoll_body_id_from_name(name: &str) -> Option<RagdollBodyId> {
    let short = name.rsplit(':').next().unwrap_or(name);
    match short {
        "Hips" => Some(RagdollBodyId::Pelvis),
        "Spine" => Some(RagdollBodyId::SpineLower),
        // Mixamo rigs typically have Spine -> Spine1 -> Spine2.
        // Spine2 is the upper-torso/chest driver closest to shoulders.
        "Spine2" => Some(RagdollBodyId::SpineUpper),
        "Spine1" => Some(RagdollBodyId::SpineUpper),
        "Head" => Some(RagdollBodyId::Head),
        "LeftArm" => Some(RagdollBodyId::UpperArmL),
        "RightArm" => Some(RagdollBodyId::UpperArmR),
        "LeftForeArm" => Some(RagdollBodyId::ForearmL),
        "RightForeArm" => Some(RagdollBodyId::ForearmR),
        "LeftUpLeg" => Some(RagdollBodyId::ThighL),
        "RightUpLeg" => Some(RagdollBodyId::ThighR),
        "LeftLeg" => Some(RagdollBodyId::CalfL),
        "RightLeg" => Some(RagdollBodyId::CalfR),
        _ => None,
    }
}

fn ragdoll_body_match_priority(name: &str) -> i32 {
    let short = name.rsplit(':').next().unwrap_or(name);
    match short {
        // Prefer Spine2 as upper torso/chest when both Spine1 and Spine2 exist.
        "Spine2" => 20,
        "Spine1" => 10,
        _ => 0,
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

/// Add render components and spawn the visual model when an NPC replicates in.
pub fn handle_npc_spawned(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<NpcAssets>>,
    new_npcs: Query<(Entity, &Npc, &NpcPosition, Option<&NpcRotation>), Added<Npc>>,
) {
    let Some(assets) = assets else { return };
    let now = time.elapsed_secs();

    for (entity, _npc, pos, rot) in new_npcs.iter() {
        let yaw = rot.map(|r| r.0).unwrap_or(0.0);
        // Ensure NPC entity has full spatial components for hierarchy propagation.
        // Without GlobalTransform, children with GlobalTransform trigger B0004 warnings.
        commands.entity(entity).insert((
            Transform::from_translation(pos.0),
            GlobalTransform::from_translation(pos.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
            NpcVisibilityState { visible: true },
            NpcNetSmoothing::from_sample(pos.0, yaw, now),
            NpcRagdollNetState::default(),
            NoFrustumCulling,
        ));

        let scene = assets.scene.clone();

        commands.entity(entity).with_children(|parent| {
            let mut model = parent.spawn((
                NpcModelRoot,
                NeedsNpcRigSetup,
                SceneRoot(scene),
                // NPC transform is the capsule center; drop model so feet touch ground.
                Transform::from_xyz(0.0, -NPC_HEIGHT * 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::splat(1.0)),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
                NoFrustumCulling,
            ));
            model.insert(NeedsDoubleSidedMaterials);
        });
    }
}

/// Add `AnimationPlayer` + `AnimationTarget`s to the spawned NPC hierarchy.
/// Oilman rigs use "Armature"; we also accept "Rig_Medium" for future rigs.
pub fn setup_npc_rig(
    mut commands: Commands,
    assets: Option<Res<NpcAssets>>,
    model_roots: Query<Entity, (With<NpcModelRoot>, With<NeedsNpcRigSetup>)>,
    children_q: Query<&Children>,
    names_q: Query<&Name>,
    transforms_q: Query<&Transform>,
    parents_q: Query<&ChildOf>,
) {
    let Some(assets) = assets else { return };

    for model_root in model_roots.iter() {
        // Cache the owning NPC entity (parent of NpcModelRoot).
        let Ok(owner) = parents_q.get(model_root).map(|p| p.parent()) else {
            continue;
        };

        // Find the rig root node inside the spawned scene.
        // Oilman uses "Armature"; we also accept "Rig_Medium" for future rigs.
        let mut stack: Vec<Entity> = vec![model_root];
        let mut rig_root: Option<Entity> = None;

        while let Some(e) = stack.pop() {
            if let Ok(name) = names_q.get(e) {
                if name.as_str() == "Rig_Medium" {
                    rig_root = Some(e);
                    break;
                }
                if name.as_str() == "Armature" {
                    rig_root = Some(e);
                    break;
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter());
            }
        }

        let Some(rig_root) = rig_root else {
            // Scene not spawned yet.
            continue;
        };

        commands.entity(rig_root).insert((
            NpcAnimationRoot,
            NpcRigOwner(owner),
            NpcAnimState::default(),
            NpcBoneMap::default(),
            AnimationPlayer::default(),
            AnimationGraphHandle(assets.animation_graph.clone()),
        ));

        // Generate AnimationTargets for the entire rig hierarchy.
        let Ok(root_name) = names_q.get(rig_root) else {
            commands.entity(model_root).remove::<NeedsNpcRigSetup>();
            continue;
        };

        let mut bone_map: HashMap<RagdollBodyId, (Entity, i32)> = HashMap::new();
        let mut bind_pose_all_bones: Vec<(Entity, Quat)> = Vec::new();
        let mut stack: Vec<(Entity, Vec<Name>)> = vec![(rig_root, vec![root_name.clone()])];
        while let Some((e, path)) = stack.pop() {
            commands.entity(e).insert((
                AnimationTargetId::from_names(path.iter()),
                AnimatedBy(rig_root),
            ));
            if let Ok(tf) = transforms_q.get(e) {
                bind_pose_all_bones.push((e, tf.rotation));
            }
            if let Some(current_name) = path.last() {
                if let Some(body_id) = ragdoll_body_id_from_name(current_name.as_str()) {
                    let priority = ragdoll_body_match_priority(current_name.as_str());
                    match bone_map.get(&body_id).copied() {
                        Some((_existing_entity, existing_priority))
                            if existing_priority > priority => {}
                        _ => {
                            bone_map.insert(body_id, (e, priority));
                        }
                    }
                }
            }

            if let Ok(children) = children_q.get(e) {
                for child in children.iter() {
                    let mut child_path = path.clone();
                    if let Ok(child_name) = names_q.get(child) {
                        child_path.push(child_name.clone());
                    }
                    stack.push((child, child_path));
                }
            }
        }
        let finalized_bones = bone_map
            .into_iter()
            .map(|(body, (entity, _priority))| (body, entity))
            .collect::<HashMap<_, _>>();
        let missing = expected_ragdoll_bodies()
            .into_iter()
            .filter(|body| !finalized_bones.contains_key(body))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            debug!(
                "NPC rig setup: complete Oilman ragdoll bone map for {:?} ({} bones)",
                owner,
                finalized_bones.len()
            );
        } else {
            warn!(
                "NPC rig setup: missing ragdoll bones for {:?}: {:?} (found {} of 12)",
                owner,
                missing,
                finalized_bones.len()
            );
        }
        let body_local_rotations = finalized_bones
            .iter()
            .filter_map(|(body, entity)| {
                transforms_q
                    .get(*entity)
                    .ok()
                    .map(|tf| (*body, tf.rotation))
            })
            .collect();
        commands.entity(rig_root).insert(NpcBoneMap {
            bones: finalized_bones,
        });
        commands.entity(rig_root).insert(NpcBindPose {
            all_bones: bind_pose_all_bones,
            body_local_rotations,
        });

        commands.entity(model_root).remove::<NeedsNpcRigSetup>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ragdoll_name_mapping_handles_mixamo_prefix() {
        assert_eq!(
            ragdoll_body_id_from_name("mixamorig:Hips"),
            Some(RagdollBodyId::Pelvis)
        );
        assert_eq!(
            ragdoll_body_id_from_name("mixamorig:LeftForeArm"),
            Some(RagdollBodyId::ForearmL)
        );
        assert_eq!(ragdoll_body_id_from_name("UnknownBone"), None);
    }
}
