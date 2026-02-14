//! spawn systems.

use super::*;

/// Add render components and spawn the visual model when an NPC replicates in.
pub fn handle_npc_spawned(
    mut commands: Commands,
    assets: Option<Res<NpcAssets>>,
    new_npcs: Query<(Entity, &Npc, &NpcPosition), Added<Npc>>,
) {
    let Some(assets) = assets else { return };

    for (entity, _npc, pos) in new_npcs.iter() {
        // Ensure NPC entity has full spatial components for hierarchy propagation.
        // Without GlobalTransform, children with GlobalTransform trigger B0004 warnings.
        commands.entity(entity).insert((
            Transform::from_translation(pos.0),
            GlobalTransform::from_translation(pos.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
            NpcVisibilityState { visible: true },
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
            AnimationPlayer::default(),
            AnimationGraphHandle(assets.animation_graph.clone()),
        ));

        // Generate AnimationTargets for the entire rig hierarchy.
        let Ok(root_name) = names_q.get(rig_root) else {
            commands.entity(model_root).remove::<NeedsNpcRigSetup>();
            continue;
        };

        let mut stack: Vec<(Entity, Vec<Name>)> = vec![(rig_root, vec![root_name.clone()])];
        while let Some((e, path)) = stack.pop() {
            commands.entity(e).insert((
                AnimationTargetId::from_names(path.iter()),
                AnimatedBy(rig_root),
            ));

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

        commands.entity(model_root).remove::<NeedsNpcRigSetup>();
    }
}
