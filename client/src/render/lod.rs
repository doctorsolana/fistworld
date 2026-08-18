use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum LodDebugAction {
    Normal,
    ForceVisible,
    ForceHidden,
}

pub(crate) fn apply_lod_visibility(
    commands: &mut Commands,
    entity: Entity,
    range: Option<VisibilityRange>,
    debug_action: LodDebugAction,
) {
    match debug_action {
        LodDebugAction::ForceHidden => {
            commands.entity(entity).insert(Visibility::Hidden);
            commands.entity(entity).remove::<VisibilityRange>();
        }
        LodDebugAction::ForceVisible => {
            commands.entity(entity).insert(Visibility::Inherited);
            commands.entity(entity).remove::<VisibilityRange>();
        }
        LodDebugAction::Normal => {
            commands.entity(entity).insert(Visibility::Inherited);
            if let Some(range) = range {
                commands.entity(entity).insert(range);
            } else {
                commands.entity(entity).remove::<VisibilityRange>();
            }
        }
    }
}
