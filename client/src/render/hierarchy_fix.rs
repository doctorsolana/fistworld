use bevy::prelude::*;
use std::collections::HashSet;

fn queue_parent_fix(commands: &mut Commands, parent: Entity) {
    commands
        .entity(parent)
        .queue_silenced(|mut parent: bevy::ecs::world::EntityWorldMut| {
            if !parent.contains::<Visibility>() {
                parent.insert(Visibility::Inherited);
            }
            if !parent.contains::<InheritedVisibility>() {
                parent.insert(InheritedVisibility::default());
            }
            if !parent.contains::<ViewVisibility>() {
                parent.insert(ViewVisibility::default());
            }
            // Insert missing spatial components independently so we never overwrite an
            // existing Transform with an unexpected default.
            if !parent.contains::<Transform>() {
                parent.insert(Transform::default());
            }
            if !parent.contains::<GlobalTransform>() {
                parent.insert(GlobalTransform::default());
            }
        });
}

/// Immediate fix: whenever a child relationship is added, ensure the parent has the
/// required transform/visibility propagation components.
///
/// This runs at `ChildOf` insertion time (earlier than the PostUpdate backfill system),
/// which reduces chance of transient B0004 warnings during scene instantiation.
pub fn ensure_parent_components_on_child_add(
    trigger: On<Add, ChildOf>,
    mut commands: Commands,
    child_of_q: Query<&ChildOf>,
) {
    let child = trigger.entity;
    let Ok(child_of) = child_of_q.get(child) else {
        return;
    };
    queue_parent_fix(&mut commands, child_of.parent());
}

/// Optional audit helper:
/// `FISTFORCE_HIERARCHY_AUDIT=1` logs each unique `(child,parent)` pair where the child has
/// `GlobalTransform` but parent is missing it, with entity names if available.
pub fn ensure_hierarchy_parent_audit(
    children_with_global: Query<(Entity, &ChildOf, Option<&Name>), With<GlobalTransform>>,
    parent_has_global: Query<(), With<GlobalTransform>>,
    parent_has_transform: Query<(), With<Transform>>,
    parent_has_visibility: Query<(), With<Visibility>>,
    parent_has_inherited_visibility: Query<(), With<InheritedVisibility>>,
    parent_has_node: Query<(), With<Node>>,
    names: Query<&Name>,
    mut enabled: Local<Option<bool>>,
    mut reported: Local<HashSet<(Entity, Entity)>>,
) {
    let enabled = *enabled.get_or_insert_with(|| {
        std::env::var("FISTFORCE_HIERARCHY_AUDIT")
            .map(|v| v == "1")
            .unwrap_or(false)
    });
    if !enabled {
        return;
    }

    for (child, child_of, child_name) in children_with_global.iter() {
        let parent = child_of.parent();
        if parent_has_global.get(parent).is_ok() {
            continue;
        }
        if !reported.insert((child, parent)) {
            continue;
        }

        let child_name = child_name
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("Entity {child}"));
        let parent_name = names
            .get(parent)
            .map(|n| n.to_string())
            .unwrap_or_else(|_| format!("Entity {parent}"));

        warn!(
            "Hierarchy audit: child {} has GlobalTransform but parent {} is missing GlobalTransform \
             [parent has Transform={}, Visibility={}, InheritedVisibility={}, Node={}]",
            child_name,
            parent_name,
            parent_has_transform.get(parent).is_ok(),
            parent_has_visibility.get(parent).is_ok(),
            parent_has_inherited_visibility.get(parent).is_ok(),
            parent_has_node.get(parent).is_ok(),
        );
    }
}

/// Detailed B0004 trace helper:
/// `FISTFORCE_HIERARCHY_TRACE=1` logs extra context at the exact moment a
/// `GlobalTransform` is inserted under a parent that does not yet have one.
pub fn update_b0004_global_trace(
    trigger: On<Add, GlobalTransform>,
    child_of_q: Query<&ChildOf>,
    names: Query<&Name>,
    has_global: Query<(), With<GlobalTransform>>,
    has_transform: Query<(), With<Transform>>,
    has_visibility: Query<(), With<Visibility>>,
    has_inherited_visibility: Query<(), With<InheritedVisibility>>,
    has_node: Query<(), With<Node>>,
    has_scene_root: Query<(), With<WorldAssetRoot>>,
    mut enabled: Local<Option<bool>>,
    mut reported: Local<HashSet<(Entity, Entity)>>,
) {
    let enabled = *enabled.get_or_insert_with(|| {
        std::env::var("FISTFORCE_HIERARCHY_TRACE")
            .map(|v| v == "1")
            .unwrap_or(false)
    });
    if !enabled {
        return;
    }

    let child = trigger.entity;
    let Ok(child_of) = child_of_q.get(child) else {
        return;
    };
    let parent = child_of.parent();
    if has_global.get(parent).is_ok() {
        return;
    }
    if !reported.insert((child, parent)) {
        return;
    }

    let fmt_entity = |e: Entity| -> String {
        names
            .get(e)
            .map(|n| format!("{} ({})", n.as_str(), e))
            .unwrap_or_else(|_| format!("{e}"))
    };

    let mut lineage = vec![fmt_entity(child), fmt_entity(parent)];
    let mut cursor = parent;
    for _ in 0..8 {
        let Ok(next) = child_of_q.get(cursor) else {
            break;
        };
        cursor = next.parent();
        lineage.push(fmt_entity(cursor));
    }

    warn!(
        "Hierarchy trace[B0004] child={} parent={} | child: transform={} visibility={} inherited_visibility={} node={} scene_root={} | parent: transform={} visibility={} inherited_visibility={} node={} scene_root={} | lineage={}",
        fmt_entity(child),
        fmt_entity(parent),
        has_transform.get(child).is_ok(),
        has_visibility.get(child).is_ok(),
        has_inherited_visibility.get(child).is_ok(),
        has_node.get(child).is_ok(),
        has_scene_root.get(child).is_ok(),
        has_transform.get(parent).is_ok(),
        has_visibility.get(parent).is_ok(),
        has_inherited_visibility.get(parent).is_ok(),
        has_node.get(parent).is_ok(),
        has_scene_root.get(parent).is_ok(),
        lineage.join(" <- "),
    );
}

/// Ensures hierarchy parents have transform/visibility propagation components so child
/// entities don't trigger B0004 warnings (`InheritedVisibility` or `GlobalTransform`
/// parent missing).
///
/// Runs a full one-time backfill, then incremental fixes for new parents.
pub fn ensure_hierarchy_visibility_parents(
    mut commands: Commands,
    all_children: Query<&ChildOf>,
    new_children: Query<(Entity, &ChildOf), Added<ChildOf>>,
    child_has_visibility: Query<(), With<InheritedVisibility>>,
    child_has_transform: Query<(), With<GlobalTransform>>,
    mut did_full_backfill: Local<bool>,
) {
    if !*did_full_backfill {
        for child_of in all_children.iter() {
            queue_parent_fix(&mut commands, child_of.parent());
        }
        *did_full_backfill = true;
    }

    for (child, child_of) in new_children.iter() {
        if child_has_visibility.get(child).is_ok() || child_has_transform.get(child).is_ok() {
            queue_parent_fix(&mut commands, child_of.parent());
        }
    }
}
