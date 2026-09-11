//! The selected-character HUD is a consumer of the shared portrait service.

use bevy::prelude::*;
use shared::components::{CharacterKind, HeroOutfit};

use crate::{
    selection::Selection,
    states::GameState,
    ui::portraits::{self, OutfitPortrait, PortraitStatus},
};

#[derive(Component)]
#[require(OutfitPortrait)]
pub(super) struct PortraitImage;

/// Readiness of the HUD's selection image, including while a modal temporarily
/// covers it. Existing captures need not guess how long a thumbnail takes.
#[derive(Resource, Default)]
pub(crate) struct PortraitReadiness(pub bool);

pub(super) fn install(app: &mut App) {
    portraits::install(app);
    app.init_resource::<PortraitReadiness>()
        .add_systems(
            Update,
            sync_selection
                .after(crate::selection::SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(OnExit(GameState::Playing), clear_readiness);
}

fn sync_selection(
    selection: Res<Selection>,
    people: Query<&HeroOutfit, With<CharacterKind>>,
    mut widgets: Query<(&mut OutfitPortrait, &PortraitStatus), With<PortraitImage>>,
    mut readiness: ResMut<PortraitReadiness>,
) {
    let wanted = selection
        .primary()
        .filter(|_| selection.len() == 1)
        .and_then(|entity| people.get(entity).ok())
        .copied();
    let mut ready = false;
    for (mut request, status) in &mut widgets {
        if request.0 != wanted {
            request.0 = wanted;
        } else {
            ready |= wanted.is_some() && status.ready;
        }
    }
    readiness.0 = ready;
}

fn clear_readiness(mut ready: ResMut<PortraitReadiness>) {
    ready.0 = false;
}
