//! ranges systems.

use super::*;

/// Update prop visibility ranges when graphics settings change.
pub(crate) fn update_prop_visibility_ranges(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    debug_mode: Res<PropLodDebugMode>,
    roots: Query<(Entity, &PropRenderTuning, &PropKindTag), With<EnvironmentProp>>,
    parents: Query<&ChildOf>,
    children_q: Query<&Children>,
    meshes_q: Query<(), With<Mesh3d>>,
    lod_presence_q: Query<&PropLodPresence>,
    names: Query<&Name>,
) {
    if !settings.is_changed() && !debug_mode.is_changed() {
        return;
    }
    let prop_multiplier = settings.prop_render_multiplier;
    let max_prop_distance = settings.view_distance as f32 * CHUNK_SIZE;

    for (root, tuning, kind) in roots.iter() {
        // Skip anything the swap-mesh LOD system owns -- it sets visibility and
        // shadows itself, and a second VisibilityRange on the same root fights
        // it. Keyed on the kind rather than on the component because this runs
        // before the handles are attached on the first frame.
        if is_tree_kind(kind.0) || crate::props::uses_swap_mesh_lod(kind.0) {
            continue;
        }
        let presence = lod_presence_q.get(root).copied().unwrap_or_default();
        update_prop_ranges_for_root(
            root,
            *tuning,
            prop_multiplier,
            max_prop_distance,
            &children_q,
            &parents,
            &names,
            &meshes_q,
            &mut commands,
            presence,
            Some(kind.0),
            *debug_mode,
        );
    }
}

pub(super) fn build_prop_visibility_range(
    tuning: PropRenderTuning,
    prop_multiplier: f32,
    max_prop_distance: f32,
    lod_level: Option<PropLodLevel>,
    fade_distance: f32,
) -> Option<VisibilityRange> {
    let base_end = tuning.visible_end_distance.map(|end| end * prop_multiplier);
    let mut lod_end = base_end.unwrap_or(PROP_LOD1_END_FALLBACK * prop_multiplier);
    lod_end = lod_end.min(max_prop_distance);
    if lod_end <= 0.0 {
        return None;
    }

    let split = base_end
        .map(|end| end * PROP_LOD1_SPLIT_RATIO)
        .unwrap_or(PROP_LOD1_START_FALLBACK * prop_multiplier)
        .min(lod_end);

    let mut fade = fade_distance * prop_multiplier;
    if fade < 0.01 {
        fade = 0.0;
    }

    match lod_level {
        Some(PropLodLevel::Lod0) => {
            let end = split;
            if end <= 0.0 {
                return None;
            }
            let end_margin = end..end;
            Some(VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin,
                use_aabb: false,
            })
        }
        Some(PropLodLevel::Lod1) => {
            let start = if fade > 0.0 {
                (split - fade).max(0.0)
            } else {
                split
            };
            if lod_end <= start {
                return None;
            }
            let start_margin = if fade > 0.0 {
                start..(start + fade).min(lod_end)
            } else {
                start..start
            };
            let end_margin = if fade > 0.0 {
                (lod_end - fade).max(start)..lod_end
            } else {
                lod_end..lod_end
            };
            Some(VisibilityRange {
                start_margin,
                end_margin,
                use_aabb: false,
            })
        }
        None => {
            let end_margin = if fade > 0.0 {
                (lod_end - fade).max(0.0)..lod_end
            } else {
                lod_end..lod_end
            };
            Some(VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin,
                use_aabb: false,
            })
        }
    }
}

pub(super) fn update_prop_ranges_for_root(
    root: Entity,
    tuning: PropRenderTuning,
    prop_multiplier: f32,
    max_prop_distance: f32,
    children_q: &Query<&Children>,
    parents: &Query<&ChildOf>,
    names: &Query<&Name>,
    meshes_q: &Query<(), With<Mesh3d>>,
    commands: &mut Commands,
    presence: PropLodPresence,
    kind: Option<shared::props::PropKind>,
    debug_mode: PropLodDebugMode,
) {
    let use_root_range = !presence.has_lod0 && !presence.has_lod1;
    if use_root_range {
        let range = build_prop_visibility_range(
            tuning,
            prop_multiplier,
            max_prop_distance,
            None,
            prop_fade_distance(kind, None),
        );
        commands.entity(root).insert(Visibility::Visible);
        if let Some(range) = range {
            commands.entity(root).insert(range);
        } else {
            commands.entity(root).remove::<VisibilityRange>();
        }
    }

    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if meshes_q.get(entity).is_ok() {
            let lod_level = detect_prop_lod_level(entity, parents, names);
            let effective_lod = adjust_lod_level(lod_level, presence);
            let skip_ranges = use_root_range && effective_lod.is_none();
            apply_prop_visibility_for_mesh(
                entity,
                tuning,
                prop_multiplier,
                max_prop_distance,
                kind,
                lod_level,
                effective_lod,
                debug_mode,
                skip_ranges,
                commands,
            );

            // Performance: disable shadow casting for far LODs.
            if !tuning.casts_shadows || matches!(effective_lod, Some(PropLodLevel::Lod1)) {
                commands.entity(entity).insert(NotShadowCaster);
            } else {
                commands.entity(entity).remove::<NotShadowCaster>();
            }
        }

        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                stack.push(child);
            }
        }
    }
}

pub(super) fn prop_fade_distance(
    _kind: Option<shared::props::PropKind>,
    lod_level: Option<PropLodLevel>,
) -> f32 {
    let _ = lod_level;
    PROP_LOD_FADE_DISTANCE
}
