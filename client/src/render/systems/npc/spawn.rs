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
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_npcs: Query<(Entity, &Npc, &NpcPosition, Option<&NpcRotation>), Added<Npc>>,
) {
    let Some(assets) = assets else { return };
    let now = time.elapsed_secs();

    for (entity, npc, pos, rot) in new_npcs.iter() {
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

        // Reference dummy: gray primitives built 1:1 from the shared ragdoll
        // body table — no skeleton, no GLB, nothing to mis-map.
        if matches!(
            npc.archetype,
            shared::components::NpcArchetype::Dummy | shared::components::NpcArchetype::CombatDummy
        ) {
            spawn_dummy_body_parts(
                &mut commands,
                entity,
                npc.archetype,
                &mut meshes,
                &mut materials,
            );
            continue;
        }

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

/// Build the reference Dummy's visual: one gray primitive per ragdoll body,
/// with shapes/orientations matching the server colliders exactly. While the
/// dummy is alive they sit in the table's T-pose layout under the NPC root;
/// during ragdoll each part is driven directly from its streamed body pose.
fn spawn_dummy_body_parts(
    commands: &mut Commands,
    npc_entity: Entity,
    archetype: shared::components::NpcArchetype,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    use shared::npc::{
        humanoid_body_part, humanoid_body_shape, ragdoll_body_axis, HumanoidBodyShape,
        HUMANOID_RAGDOLL_BODIES,
    };
    use shared::protocol::RagdollBodyId as B;
    use shared::weapons::damage::HitZone;

    let gray = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.62, 0.65),
        perceptual_roughness: 0.85,
        metallic: 0.0,
        ..Default::default()
    });
    let head = materials.add(StandardMaterial {
        base_color: Color::srgb(0.70, 0.30, 0.24),
        perceptual_roughness: 0.82,
        ..Default::default()
    });
    let chest = materials.add(StandardMaterial {
        base_color: Color::srgb(0.23, 0.34, 0.42),
        perceptual_roughness: 0.78,
        ..Default::default()
    });
    let abdomen = materials.add(StandardMaterial {
        base_color: Color::srgb(0.34, 0.39, 0.29),
        perceptual_roughness: 0.82,
        ..Default::default()
    });
    let arms = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.40, 0.23),
        perceptual_roughness: 0.84,
        ..Default::default()
    });
    let legs = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.30, 0.34),
        perceptual_roughness: 0.86,
        ..Default::default()
    });
    let nose_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.18),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..Default::default()
    });
    let nose_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.10));

    commands.entity(npc_entity).with_children(|parent| {
        for def in HUMANOID_RAGDOLL_BODIES.iter() {
            let mesh = match humanoid_body_shape(def.id) {
                HumanoidBodyShape::Sphere { radius } => meshes.add(Sphere::new(radius)),
                HumanoidBodyShape::Capsule {
                    half_segment,
                    radius,
                } => meshes.add(Capsule3d::new(radius, half_segment * 2.0)),
                HumanoidBodyShape::Cuboid { half_extents } => meshes.add(Cuboid::new(
                    half_extents.x * 2.0,
                    half_extents.y * 2.0,
                    half_extents.z * 2.0,
                )),
            };
            let material = if archetype == shared::components::NpcArchetype::CombatDummy {
                match humanoid_body_part(def.id).hit_zone() {
                    HitZone::Head => head.clone(),
                    HitZone::Chest => chest.clone(),
                    HitZone::Stomach => abdomen.clone(),
                    HitZone::Arms => arms.clone(),
                    HitZone::Legs => legs.clone(),
                }
            } else {
                gray.clone()
            };

            let mut part = parent.spawn((
                NpcDummyBody { body: def.id },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(def.local_offset)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, ragdoll_body_axis(def))),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
                NoFrustumCulling,
            ));

            // Facial marker makes facing and inversion visible during ragdoll.
            if def.id == B::Head {
                part.with_children(|head| {
                    head.spawn((
                        Mesh3d(nose_mesh.clone()),
                        MeshMaterial3d(nose_material.clone()),
                        Transform::from_xyz(0.0, 0.02, -0.17),
                        GlobalTransform::default(),
                        Visibility::Inherited,
                        InheritedVisibility::default(),
                        NoFrustumCulling,
                    ));
                });
            }
        }
    });
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

        // Compose the transform chain from the NPC root entity down to (but
        // excluding) the rig root: NpcModelRoot (feet offset + PI yaw) plus
        // any intermediate scene nodes. Needed so ragdoll math can relate
        // bind-pose bone frames to the NPC root frame.
        let mut prefix_chain: Vec<Entity> = Vec::new();
        let mut cursor = rig_root;
        while let Ok(parent) = parents_q.get(cursor).map(|p| p.parent()) {
            if parent == owner {
                break;
            }
            prefix_chain.push(parent);
            cursor = parent;
        }
        let mut prefix = Transform::IDENTITY;
        for entity in prefix_chain.iter().rev() {
            if let Ok(tf) = transforms_q.get(*entity) {
                prefix = prefix.mul_transform(*tf);
            }
        }

        let mut bone_map: HashMap<RagdollBodyId, (Entity, i32)> = HashMap::new();
        let mut rig_bones: Vec<RagdollRigBone> = Vec::new();
        // DFS stack: (entity, name path, parent index in rig_bones, parent rel transform)
        let mut stack: Vec<(Entity, Vec<Name>, Option<usize>, Transform)> =
            vec![(rig_root, vec![root_name.clone()], None, prefix)];
        while let Some((e, path, parent_idx, parent_rel)) = stack.pop() {
            commands.entity(e).insert((
                AnimationTargetId::from_names(path.iter()),
                AnimatedBy(rig_root),
            ));
            let bind_local = transforms_q.get(e).copied().unwrap_or_default();
            let rel = parent_rel.mul_transform(bind_local);
            let bone_index = rig_bones.len();
            rig_bones.push(RagdollRigBone {
                entity: e,
                parent: parent_idx,
                body: None, // resolved below once name priorities settle
                bind_local,
                bind_rel: rel,
            });
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
                    stack.push((child, child_path, Some(bone_index), rel));
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

        // Back-fill which streamed body drives each bone.
        let body_by_entity: HashMap<Entity, RagdollBodyId> = finalized_bones
            .iter()
            .map(|(body, entity)| (*entity, *body))
            .collect();
        for bone in rig_bones.iter_mut() {
            bone.body = body_by_entity.get(&bone.entity).copied();
        }

        // Hips anchor: bind translation of the pelvis bone relative to the
        // NPC root (recompute the rel chain for just that bone).
        let hips_index = rig_bones
            .iter()
            .position(|bone| bone.body == Some(RagdollBodyId::Pelvis));
        let hips_offset_from_root = hips_index
            .map(|index| {
                let mut chain = Vec::new();
                let mut cursor = Some(index);
                while let Some(i) = cursor {
                    chain.push(i);
                    cursor = rig_bones[i].parent;
                }
                let mut rel = prefix;
                for i in chain.iter().rev() {
                    rel = rel.mul_transform(rig_bones[*i].bind_local);
                }
                rel.translation
            })
            .unwrap_or(Vec3::ZERO);

        commands.entity(rig_root).insert(NpcBoneMap {
            bones: finalized_bones,
        });
        commands.entity(rig_root).insert(NpcRagdollRig {
            prefix,
            bones: rig_bones,
            hips_offset_from_root,
            hips_index,
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
