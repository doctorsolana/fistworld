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

/// Above this zoom, prop shadows are sub-texel specks in the zoom-stretched
/// cascades — and skipping thousands of tree meshes in every cascade pass is
/// the main thing that keeps far-zoom shadow rendering affordable.
const PROP_SHADOWS_OFF_ZOOM: f32 = 1400.0;
/// Re-enable threshold. The gap is hysteresis so a camera hovering at the
/// boundary doesn't sweep the prop set every frame.
const PROP_SHADOWS_ON_ZOOM: f32 = 1000.0;
/// While casting is off, newly streamed props must inherit the gate; a slow
/// idempotent re-sweep catches them (ZST re-inserts cause no archetype moves).
const GATE_RESWEEP_SECS: f32 = 1.0;

#[derive(Resource, Default)]
pub struct PropShadowGate {
    casting_disabled: bool,
    resweep_timer: f32,
}

/// Zoom-gate shadow casting for environment props (trees, bushes, rocks).
pub fn gate_prop_shadow_casters(
    time: Res<Time>,
    mut gate: ResMut<PropShadowGate>,
    cameras: Query<&crate::camera_rts::CommanderCamera>,
    roots: Query<Entity, With<crate::props::EnvironmentProp>>,
    children_q: Query<&Children>,
    mesh_q: Query<(), With<Mesh3d>>,
    mut commands: Commands,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };

    let disable = if gate.casting_disabled {
        camera.zoom > PROP_SHADOWS_ON_ZOOM
    } else {
        camera.zoom > PROP_SHADOWS_OFF_ZOOM
    };

    let toggled = disable != gate.casting_disabled;
    let resweep = if disable && !toggled {
        gate.resweep_timer += time.delta_secs();
        gate.resweep_timer >= GATE_RESWEEP_SECS
    } else {
        false
    };
    if !toggled && !resweep {
        return;
    }

    gate.casting_disabled = disable;
    gate.resweep_timer = 0.0;
    for root in roots.iter() {
        apply_shadow_caster_for_children(root, !disable, &children_q, &mesh_q, &mut commands);
    }
}
