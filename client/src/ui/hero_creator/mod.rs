//! Retained hero wardrobe: a live idle portrait and the shared medieval ledger.
//! Layout, input/server intent and the camera-following preview have separate owners.

mod actions;
mod artwork;
mod layout;
mod preview;

use crate::states::GameState;
use actions::*;
use bevy::prelude::*;

pub use actions::CreatorClickGuard;
pub(crate) use artwork::CreatorArtwork;

pub struct HeroCreatorPlugin;

impl Plugin for HeroCreatorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeroCreatorOpen>()
            .init_resource::<CreatorClickGuard>()
            .init_resource::<CreatorFeedback>()
            .init_resource::<CreatorFocus>()
            .init_resource::<CreatorArtwork>()
            .add_systems(
                Update,
                (
                    update_click_guard
                        .before(handle_arrow_buttons)
                        .before(handle_confirm_buttons)
                        .before(handle_developer_skip),
                    layout::spawn_creator.run_if(creator_open),
                    release_creator_focus
                        .after(handle_confirm_buttons)
                        .after(handle_developer_skip),
                    layout::despawn_creator
                        .after(release_creator_focus)
                        .run_if(creator_closed),
                    (
                        handle_arrow_buttons
                            .before(sync_slot_labels)
                            .before(sync_preview_outfit),
                        handle_confirm_buttons,
                        handle_developer_skip,
                        sync_slot_labels,
                        sync_preview_outfit,
                        sync_status_text.after(handle_confirm_buttons),
                    )
                        .run_if(creator_open),
                    sync_creator_open_state
                        .after(handle_confirm_buttons)
                        .after(handle_developer_skip),
                )
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                initialize_creator_focus
                    .after(bevy::ui::UiSystems::Layout)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                OnExit(GameState::Playing),
                (
                    force_close_creator,
                    release_creator_focus,
                    layout::despawn_creator,
                    sync_creator_open_state,
                )
                    .chain(),
            );
        preview::configure(app);
    }
}

#[derive(Resource, Default)]
pub struct HeroCreatorOpen(pub bool);

fn creator_open(open: Res<HeroCreatorOpen>) -> bool {
    open.0
}
fn creator_closed(open: Res<HeroCreatorOpen>) -> bool {
    !open.0
}

#[derive(Component)]
struct CreatorRoot;
#[derive(Component)]
struct CreatorPanel;

/// Manifest indices stay stable even when layout groups appearance before clothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreatorRow {
    Slot(usize),
    Skin,
}
#[derive(Component, Clone, Copy)]
struct ArrowButton {
    row: CreatorRow,
    dir: i8,
}
#[derive(Component, Clone, Copy)]
struct SlotValueText(CreatorRow);
#[derive(Component)]
struct BeginJourneyButton;
#[derive(Component)]
struct CreatorStatusText;
