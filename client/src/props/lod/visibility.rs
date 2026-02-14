//! visibility systems.

use super::*;

fn lod_debug_action(mode: PropLodDebugMode, lod_level: Option<PropLodLevel>) -> LodDebugAction {
    match mode {
        PropLodDebugMode::Off => LodDebugAction::Normal,
        PropLodDebugMode::ForceLod0 => match lod_level {
            Some(PropLodLevel::Lod0) => LodDebugAction::ForceVisible,
            Some(PropLodLevel::Lod1) => LodDebugAction::ForceHidden,
            None => LodDebugAction::Normal,
        },
        PropLodDebugMode::ForceLod1 => match lod_level {
            Some(PropLodLevel::Lod1) => LodDebugAction::ForceVisible,
            Some(PropLodLevel::Lod0) => LodDebugAction::ForceHidden,
            None => LodDebugAction::Normal,
        },
    }
}

pub(super) fn apply_prop_visibility_for_mesh(
    entity: Entity,
    tuning: PropRenderTuning,
    prop_multiplier: f32,
    max_prop_distance: f32,
    kind: Option<shared::props::PropKind>,
    lod_level: Option<PropLodLevel>,
    effective_lod: Option<PropLodLevel>,
    debug_mode: PropLodDebugMode,
    use_root_range: bool,
    commands: &mut Commands,
) {
    if use_root_range {
        commands.entity(entity).insert(Visibility::Inherited);
        commands.entity(entity).remove::<VisibilityRange>();
        commands.entity(entity).insert(PropVisibilityReady);
        return;
    }
    let debug_action = lod_debug_action(debug_mode, lod_level);
    let range = if matches!(debug_action, LodDebugAction::Normal) {
        let fade_distance = prop_fade_distance(kind, effective_lod);
        build_prop_visibility_range(
            tuning,
            prop_multiplier,
            max_prop_distance,
            effective_lod,
            fade_distance,
        )
    } else {
        None
    };

    apply_lod_visibility(commands, entity, range, debug_action);
    commands.entity(entity).insert(PropVisibilityReady);
}

pub(super) fn apply_tree_mesh_defaults(
    entity: Entity,
    tuning: PropRenderTuning,
    commands: &mut Commands,
) {
    commands.entity(entity).insert(Visibility::Inherited);
    commands.entity(entity).remove::<VisibilityRange>();
    commands.entity(entity).insert(PropVisibilityReady);
    if !tuning.casts_shadows {
        commands.entity(entity).insert(NotShadowCaster);
    }
}

/// Reveal prop roots only after all mesh children have visibility applied.
pub(crate) fn reveal_pending_prop_roots(
    mut commands: Commands,
    roots: Query<Entity, With<PendingPropVisibility>>,
    children_q: Query<&Children>,
    mesh_q: Query<(), With<Mesh3d>>,
    ready_q: Query<(), With<PropVisibilityReady>>,
) {
    for root in roots.iter() {
        let mut stack = vec![root];
        let mut all_ready = true;

        while let Some(entity) = stack.pop() {
            if mesh_q.get(entity).is_ok() && ready_q.get(entity).is_err() {
                all_ready = false;
                break;
            }

            if let Ok(children) = children_q.get(entity) {
                for child in children.iter() {
                    stack.push(child);
                }
            }
        }

        if all_ready {
            commands
                .entity(root)
                .insert(Visibility::Visible)
                .remove::<PendingPropVisibility>();
        }
    }
}

/// Manual LOD switching for tree props to avoid dither flicker with instancing.
pub(crate) fn update_tree_lod_visibility(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    debug_mode: Res<PropLodDebugMode>,
    player: Query<&PlayerPosition, With<LocalPlayer>>,
    mut roots: Query<
        (
            Entity,
            &GlobalTransform,
            &PropRenderTuning,
            &TreeLodEntities,
            Option<&TreeLodRuntimeState>,
            &mut Visibility,
        ),
        (With<TreeLodRoot>, Without<PendingPropVisibility>),
    >,
) {
    let Ok(player_pos) = player.single() else {
        return;
    };
    let prop_multiplier = settings.prop_render_multiplier;
    let max_prop_distance = settings.view_distance as f32 * CHUNK_SIZE;

    for (root, transform, tuning, lods, runtime_state, mut root_visibility) in roots.iter_mut() {
        let distance = transform.translation().distance(player_pos.0);
        let base_end = tuning.visible_end_distance.map(|end| end * prop_multiplier);
        let mut lod_end = base_end.unwrap_or(PROP_LOD1_END_FALLBACK * prop_multiplier);
        lod_end = lod_end.min(max_prop_distance);
        if lod_end <= 0.0 {
            continue;
        }

        let split = base_end
            .map(|end| end * PROP_LOD1_SPLIT_RATIO)
            .unwrap_or(PROP_LOD1_START_FALLBACK * prop_multiplier)
            .min(lod_end);

        let current_state = runtime_state.copied().unwrap_or_default();
        let has_lod1 = lods.lod1.is_some();
        let desired_active_lod = match *debug_mode {
            PropLodDebugMode::ForceLod0 => TreeActiveLod::Lod0,
            PropLodDebugMode::ForceLod1 => {
                if has_lod1 {
                    TreeActiveLod::Lod1
                } else {
                    TreeActiveLod::Lod0
                }
            }
            PropLodDebugMode::Off => {
                if distance > lod_end {
                    TreeActiveLod::Hidden
                } else if !has_lod1 {
                    TreeActiveLod::Lod0
                } else {
                    match current_state.active_lod {
                        TreeActiveLod::Lod0 => {
                            if distance >= split + TREE_LOD_HYSTERESIS {
                                TreeActiveLod::Lod1
                            } else {
                                TreeActiveLod::Lod0
                            }
                        }
                        TreeActiveLod::Lod1 => {
                            if distance <= split - TREE_LOD_HYSTERESIS {
                                TreeActiveLod::Lod0
                            } else {
                                TreeActiveLod::Lod1
                            }
                        }
                        TreeActiveLod::Hidden => {
                            if distance <= split {
                                TreeActiveLod::Lod0
                            } else {
                                TreeActiveLod::Lod1
                            }
                        }
                    }
                }
            }
        };

        let desired_casts_shadows =
            tuning.casts_shadows && matches!(desired_active_lod, TreeActiveLod::Lod0);

        if current_state.active_lod == desired_active_lod
            && current_state.casts_shadows == desired_casts_shadows
        {
            continue;
        }

        if let Some(lod0) = lods.lod0 {
            commands
                .entity(lod0)
                .insert(if matches!(desired_active_lod, TreeActiveLod::Lod0) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            if desired_casts_shadows {
                commands.entity(lod0).remove::<NotShadowCaster>();
            } else {
                commands.entity(lod0).insert(NotShadowCaster);
            }
        }
        if let Some(lod1) = lods.lod1 {
            commands
                .entity(lod1)
                .insert(if matches!(desired_active_lod, TreeActiveLod::Lod1) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            // Far LODs should not cast shadows.
            commands.entity(lod1).insert(NotShadowCaster);
        }

        *root_visibility = if matches!(desired_active_lod, TreeActiveLod::Hidden) {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };

        commands.entity(root).insert(TreeLodRuntimeState {
            active_lod: desired_active_lod,
            casts_shadows: desired_casts_shadows,
        });
    }
}
