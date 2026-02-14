//! visibility systems.

use super::*;

pub(super) fn apply_shadow_caster_for_children(
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

/// Disable shadow casting for distant vehicles to reduce GPU shadow cost.
pub fn update_vehicle_shadow_culling(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut commands: Commands,
    children_q: Query<&Children>,
    mesh_q: Query<(), With<Mesh3d>>,
    mut vehicles: Query<(Entity, &Transform, Option<&VehicleShadowState>), With<VehicleVisual>>,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    for (entity, transform, state_opt) in vehicles.iter_mut() {
        let dist_sq = (transform.translation - player_pos.0).length_squared();
        let should_cast = dist_sq <= VEHICLE_SHADOW_RANGE_SQ;
        let current = state_opt.map(|s| s.enabled).unwrap_or(true);
        if state_opt.is_none() || current != should_cast {
            commands.entity(entity).insert(VehicleShadowState {
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
    }
}

/// Ensure newly spawned vehicle mesh entities inherit shadow-culling state.
pub fn apply_vehicle_shadow_state_to_new_meshes(
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<&VehicleShadowState, With<VehicleVisual>>,
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
