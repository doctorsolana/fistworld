use bevy::ecs::world::EntityWorldMut;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;


#[derive(Component, Clone, Copy, Debug)]
pub struct ShadowCullRoot {
    pub max_distance_sq: f32,
}


#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShadowCullState {
    pub enabled: bool,
}

pub fn apply_shadow_caster_for_children(
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
                commands
                    .entity(entity)
                    .queue_silenced(|mut e: EntityWorldMut| {
                        e.remove::<NotShadowCaster>();
                    });
            } else {
                commands
                    .entity(entity)
                    .queue_silenced(|mut e: EntityWorldMut| {
                        e.insert(NotShadowCaster);
                    });
            }
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter());
        }
    }
}

// NOTE: zoom-gating of tree shadow casters lives in the prop LOD system
// (props/lod/visibility.rs), which must stay the single owner of prop
// NotShadowCaster state — a second writer here caused toggle churn.
