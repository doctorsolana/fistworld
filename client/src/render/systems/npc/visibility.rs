//! visibility systems.

use super::*;

fn apply_shadow_caster_for_children(
    root: Entity,
    enabled: bool,
    children_q: &Query<&Children>,
    mesh_q: &Query<(), With<Mesh3d>>,
    commands: &mut Commands,
) {
    crate::render::shadow_cull::apply_shadow_caster_for_children(
        root, enabled, children_q, mesh_q, commands,
    );
}

fn apply_frustum_cull_override_for_children(
    root: Entity,
    enabled: bool,
    children_q: &Query<&Children>,
    mesh_q: &Query<(), With<Mesh3d>>,
    commands: &mut Commands,
) {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if mesh_q.get(entity).is_ok() {
            if enabled {
                commands.entity(entity).insert(NoFrustumCulling);
            } else {
                commands.entity(entity).remove::<NoFrustumCulling>();
            }
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
    }
}

/// Hide NPC visuals when far from the local player.
pub fn update_npc_visibility(
    time: Res<Time>,
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    settings: Res<GraphicsSettings>,
    mut commands: Commands,
    children_q: Query<&Children>,
    mesh_q: Query<(), With<Mesh3d>>,
    mut npcs: Query<
        (
            Entity,
            &Transform,
            &mut Visibility,
            Option<&NpcShadowState>,
            Option<&NpcVisibilityState>,
            Option<&NpcFrustumCullOverrideState>,
        ),
        With<Npc>,
    >,
    mut throttle: Local<f32>,
) {
    *throttle += time.delta_secs();
    const NPC_VISIBILITY_INTERVAL: f32 = 0.2;
    if *throttle < NPC_VISIBILITY_INTERVAL {
        return;
    }
    *throttle = 0.0;

    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let max_distance = settings.view_distance as f32 * CHUNK_SIZE;
    let max_distance_sq = max_distance * max_distance;

    for (entity, transform, mut visibility, shadow_state, vis_state, cull_state) in npcs.iter_mut()
    {
        let dist_sq = (transform.translation - player_pos.0).length_squared();
        let should_be_visible = dist_sq <= max_distance_sq;
        let current_visible = vis_state.map(|s| s.visible).unwrap_or(true);
        if current_visible != should_be_visible {
            *visibility = if should_be_visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            commands.entity(entity).insert(NpcVisibilityState {
                visible: should_be_visible,
            });
        }

        let should_cast = dist_sq <= NPC_SHADOW_RANGE_SQ;
        let current = shadow_state.map(|s| s.enabled).unwrap_or(true);
        if shadow_state.is_none() || current != should_cast {
            commands.entity(entity).insert(NpcShadowState {
                enabled: should_cast,
            });
            apply_shadow_caster_for_children(
                entity,
                should_cast,
                &children_q,
                &mesh_q,
                &mut commands,
            );
        }

        let should_disable_frustum_culling = dist_sq <= NPC_NO_FRUSTUM_CULL_RANGE_SQ;
        let current_cull_override = cull_state.map(|s| s.enabled).unwrap_or(false);
        if cull_state.is_none() || current_cull_override != should_disable_frustum_culling {
            commands.entity(entity).insert(NpcFrustumCullOverrideState {
                enabled: should_disable_frustum_culling,
            });
            apply_frustum_cull_override_for_children(
                entity,
                should_disable_frustum_culling,
                &children_q,
                &mesh_q,
                &mut commands,
            );
        }
    }
}

/// Ensure newly spawned NPC mesh entities inherit shadow-culling state.
pub fn apply_npc_shadow_state_to_new_meshes(
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<&NpcShadowState, With<Npc>>,
    mut commands: Commands,
) {
    for mesh_entity in new_meshes.iter() {
        let mut current = mesh_entity;
        loop {
            if let Ok(state) = roots.get(current) {
                if state.enabled {
                    commands.entity(mesh_entity).remove::<NotShadowCaster>();
                } else {
                    commands.entity(mesh_entity).insert(NotShadowCaster);
                }
                break;
            }
            let Ok(parent) = parents.get(current) else {
                break;
            };
            current = parent.parent();
        }
    }
}

/// Disable frustum culling on NPC mesh descendants.
/// This prevents close-range pop-out on animated scene meshes when camera proximity is extreme.
pub fn apply_npc_no_frustum_culling_to_new_meshes(
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<&NpcFrustumCullOverrideState, With<Npc>>,
    mut commands: Commands,
) {
    for mesh_entity in new_meshes.iter() {
        let mut current = mesh_entity;
        loop {
            if let Ok(state) = roots.get(current) {
                if state.enabled {
                    commands.entity(mesh_entity).insert(NoFrustumCulling);
                } else {
                    commands.entity(mesh_entity).remove::<NoFrustumCulling>();
                }
                break;
            }
            let Ok(parent) = parents.get(current) else {
                break;
            };
            current = parent.parent();
        }
    }
}

/// Force double-sided materials for custom NPC models (Oilman).
pub fn apply_double_sided_npc_materials(
    mut commands: Commands,
    roots: Query<Entity, With<NeedsDoubleSidedMaterials>>,
    children_q: Query<&Children>,
    materials_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in roots.iter() {
        let mut stack: Vec<Entity> = vec![root];
        let mut updated_any = false;

        while let Some(entity) = stack.pop() {
            if let Ok(handle) = materials_q.get(entity) {
                if let Some(material) = materials.get_mut(&handle.0) {
                    material.cull_mode = None;
                    material.alpha_mode = AlphaMode::Opaque;
                    // Guard against AI-pipeline GLB exports that omit
                    // metallicFactor (glTF default = 1.0): a fully-metallic
                    // rough surface has no diffuse response and renders as a
                    // dark, muddy husk no matter how strong the lighting is.
                    if material.metallic > 0.05 {
                        material.metallic = 0.0;
                    }
                    material.perceptual_roughness = material.perceptual_roughness.max(0.7);
                    material.reflectance = 0.35;
                    updated_any = true;
                }
            }
            if let Ok(children) = children_q.get(entity) {
                stack.extend(children.iter());
            }
        }

        if updated_any {
            commands.entity(root).remove::<NeedsDoubleSidedMaterials>();
        }
    }
}
