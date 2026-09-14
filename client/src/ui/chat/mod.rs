//! Server-wide chat: retained HUD presentation, draft input and wire adapters.

mod draft;
mod input;
mod network;
mod state;
mod view;

pub(crate) use input::ChatInput;
pub(crate) use state::ChatState;

use crate::states::GameState;
use bevy::{input::InputSystems, prelude::*, ui::UiSystems};

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatState>()
            .configure_sets(
                PreUpdate,
                ChatInput.after(InputSystems).after(UiSystems::Focus),
            )
            .add_systems(OnEnter(GameState::Playing), view::spawn)
            .add_systems(
                OnExit(GameState::Playing),
                (input::reset, view::despawn).chain(),
            )
            .add_systems(
                PreUpdate,
                (network::observe_connection, input::handle_keyboard)
                    .chain()
                    .in_set(ChatInput)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    network::receive,
                    network::send,
                    view::sync_visibility,
                    view::sync_history,
                    view::sync_draft,
                    view::sync_preview,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                (view::scroll_to_newest, view::fit_draft)
                    .after(UiSystems::Layout)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
