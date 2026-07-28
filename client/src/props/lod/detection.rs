//! detection systems.

use super::*;

/// Apply [`PropRenderTuning`] to newly spawned meshes under prop scene hierarchies.
pub(crate) fn apply_prop_render_tuning(
    mut commands: Commands,
    new_meshes: Query<Entity, (Added<Mesh3d>, Without<TerrainChunk>, Without<WaterChunk>)>,
    meshes_q: Query<(), With<Mesh3d>>,
    parents: Query<&ChildOf>,
    tunings: Query<&PropRenderTuning>,
    prop_kinds: Query<&PropKindTag>,
    prop_roots: Query<(), With<EnvironmentProp>>,
    tree_lod_roots: Query<(), With<TreeLodMeshHandles>>,
    children_q: Query<&Children>,
    mut lod_presence_q: Query<&mut PropLodPresence>,
    names: Query<&Name>,
    settings: Res<GraphicsSettings>,
    debug_mode: Res<PropLodDebugMode>,
) {
    let prop_multiplier = settings.prop_render_multiplier;
    let max_prop_distance = settings.view_distance as f32 * CHUNK_SIZE;

    for mesh_entity in new_meshes.iter() {
        // Walk up the hierarchy until we find an ancestor with `PropRenderTuning`.
        let mut current = mesh_entity;
        let tuning = loop {
            if let Ok(tuning) = tunings.get(current) {
                break Some(*tuning);
            }
            let Ok(parent) = parents.get(current) else {
                break None;
            };
            current = parent.parent();
        };

        let Some(tuning) = tuning else { continue };

        let lod_level = detect_prop_lod_level(mesh_entity, &parents, &names);
        let root = find_prop_root(mesh_entity, &parents, &prop_roots);
        let root_kind = root
            .and_then(|entity| prop_kinds.get(entity).ok())
            .map(|kind| kind.0);
        // "Tree defaults" (no VisibilityRange; the tree LOD system owns
        // visibility + shadows) only apply when the root actually IS managed by
        // that system. Pines are tree kinds but spawn as multi-primitive scenes
        // without TreeLodMeshHandles — routing them here left them exempt from
        // every distance/shadow rule (rendered + casting at any zoom).
        let is_tree = root_kind.map(is_tree_kind).unwrap_or(false)
            && root.is_some_and(|entity| tree_lod_roots.contains(entity));
        let mut presence_changed = false;
        let mut presence = PropLodPresence::default();
        if let Some(root) = root {
            let computed = compute_lod_presence(root, &children_q, &parents, &names, &meshes_q);
            if let Ok(mut existing) = lod_presence_q.get_mut(root) {
                presence = *existing;
                if presence != computed {
                    *existing = computed;
                    presence_changed = true;
                }
                presence = computed;
            } else {
                commands.entity(root).insert(computed);
                presence = computed;
                presence_changed = true;
            }
        }

        let effective_lod = adjust_lod_level(lod_level, presence);
        let use_root_range = !presence.has_lod0 && !presence.has_lod1;
        if is_tree {
            apply_tree_mesh_defaults(mesh_entity, tuning, &mut commands);
        } else {
            apply_prop_visibility_for_mesh(
                mesh_entity,
                tuning,
                prop_multiplier,
                max_prop_distance,
                root_kind,
                lod_level,
                effective_lod,
                *debug_mode,
                use_root_range,
                &mut commands,
            );
        }

        if !is_tree && !tuning.casts_shadows {
            commands.entity(mesh_entity).insert(NotShadowCaster);
        }

        if presence_changed && !is_tree {
            if let Some(root) = root {
                update_prop_ranges_for_root(
                    root,
                    tuning,
                    prop_multiplier,
                    max_prop_distance,
                    &children_q,
                    &parents,
                    &names,
                    &meshes_q,
                    &mut commands,
                    presence,
                    root_kind,
                    *debug_mode,
                );
            }
        }
    }
}

pub(super) fn detect_prop_lod_level(
    entity: Entity,
    parents: &Query<&ChildOf>,
    names: &Query<&Name>,
) -> Option<PropLodLevel> {
    let mut current = entity;
    loop {
        if let Ok(name) = names.get(current) {
            let lowered = name.as_str().to_ascii_lowercase();
            if contains_lod_marker(&lowered, 0) {
                return Some(PropLodLevel::Lod0);
            }
            if contains_lod_marker(&lowered, 1) {
                return Some(PropLodLevel::Lod1);
            }
        }
        let Ok(parent) = parents.get(current) else {
            break;
        };
        current = parent.parent();
    }
    None
}

fn find_prop_root(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<EnvironmentProp>>,
) -> Option<Entity> {
    let mut current = entity;
    loop {
        if roots.get(current).is_ok() {
            return Some(current);
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
}

fn compute_lod_presence(
    root: Entity,
    children_q: &Query<&Children>,
    parents: &Query<&ChildOf>,
    names: &Query<&Name>,
    meshes_q: &Query<(), With<Mesh3d>>,
) -> PropLodPresence {
    let mut presence = PropLodPresence::default();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if meshes_q.get(entity).is_ok() {
            if let Some(lod) = detect_prop_lod_level(entity, parents, names) {
                match lod {
                    PropLodLevel::Lod0 => presence.has_lod0 = true,
                    PropLodLevel::Lod1 => presence.has_lod1 = true,
                }
            }
            if presence.has_lod0 && presence.has_lod1 {
                break;
            }
        }
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                stack.push(child);
            }
        }
    }
    presence
}

pub(super) fn adjust_lod_level(
    lod_level: Option<PropLodLevel>,
    presence: PropLodPresence,
) -> Option<PropLodLevel> {
    if presence.has_lod0 && presence.has_lod1 {
        return lod_level;
    }
    None
}

fn contains_lod_marker(name: &str, level: u8) -> bool {
    let needle = format!("lod{level}");
    if name.contains(&needle) {
        return true;
    }
    let needle = format!("lod_{level}");
    if name.contains(&needle) {
        return true;
    }
    let needle = format!("lod {level}");
    if name.contains(&needle) {
        return true;
    }
    // Handle zero-padded LOD labels (e.g., LOD_01 / LOD 00 / LOD01).
    if level <= 9 {
        let needle = format!("lod0{level}");
        if name.contains(&needle) {
            return true;
        }
        let needle = format!("lod_0{level}");
        if name.contains(&needle) {
            return true;
        }
        let needle = format!("lod 0{level}");
        if name.contains(&needle) {
            return true;
        }
    }
    false
}
