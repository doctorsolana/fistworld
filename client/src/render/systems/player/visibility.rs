//! visibility systems.

use super::*;

/// Update local player model visibility.
/// In first-person: hide the model so it doesn't block the view.
/// In third-person: show the model.
pub fn update_local_player_visibility(
    mut commands: Commands,
    input_state: Res<InputState>,
    mut last_mode: ResMut<LastCameraMode>,
    local_model: Query<Entity, With<LocalPlayerModel>>,
) {
    let Ok(model_root) = local_model.single() else {
        return;
    };

    // Only update when camera mode changes
    let current_mode = input_state.camera_mode;
    if last_mode.0 == Some(current_mode) {
        return;
    }
    last_mode.0 = Some(current_mode);

    // Set visibility based on camera mode
    let visibility = match current_mode {
        CameraMode::FirstPerson => Visibility::Hidden,
        CameraMode::ThirdPerson => Visibility::Inherited,
    };

    commands.entity(model_root).insert(visibility);
}

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

/// Disable shadow casting for distant remote players to reduce GPU shadow cost.
pub fn update_player_shadow_culling(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut commands: Commands,
    children_q: Query<&Children>,
    mesh_q: Query<(), With<Mesh3d>>,
    mut players: Query<
        (
            Entity,
            &Transform,
            Option<&PlayerShadowState>,
            Option<&LocalPlayer>,
        ),
        With<Player>,
    >,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    for (entity, transform, state_opt, is_local) in players.iter_mut() {
        if is_local.is_some() {
            continue;
        }
        let dist_sq = (transform.translation - player_pos.0).length_squared();
        let should_cast = dist_sq <= PLAYER_SHADOW_RANGE_SQ;
        let current = state_opt.map(|s| s.enabled).unwrap_or(true);
        if state_opt.is_none() || current != should_cast {
            commands.entity(entity).insert(PlayerShadowState {
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

/// Ensure newly spawned player mesh entities inherit shadow-culling state.
pub fn apply_player_shadow_state_to_new_meshes(
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<&PlayerShadowState, With<Player>>,
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
