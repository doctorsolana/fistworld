//! Debug time-of-day menu (client-side)

pub mod actions;
pub mod layout;
pub mod state_sync;

use actions::{
    close_debug_menu_on_main_menu, close_debug_time_menu_on_escape, debug_menu_closed,
    debug_menu_open, handle_backdrop_click, handle_debug_menu_interactions, toggle_debug_time_menu,
};
use layout::{despawn_debug_time_menu, spawn_debug_time_menu, style_debug_time_menu};
use state_sync::{
    sync_debug_character_selection, sync_debug_menu_open_state, update_character_button_label,
    update_perf_button_labels,
};

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use lightyear::prelude::*;

use shared::components::{LocalPlayer, PlayerCharacter};
use shared::protocol::{
    ReliableChannel, SetPlayerCharacter, SetTimeOfDay, SpawnOilmanDebug, TimeOfDayPreset,
};

use crate::input::InputState;
use crate::render::systems::{CloudCover, CloudCoverMode, CloudCoverOverride};
use crate::states::GameState;
use crate::ui::modal::{handle_backdrop_pressed, spawn_modal, sync_modal_cursor, ModalLayout};
use crate::ui::styles::{
    button_style, button_text_style, ACCENT_COLOR, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED,
    TEXT_COLOR, TEXT_MUTED,
};

pub struct DebugTimeMenuPlugin;

impl Plugin for DebugTimeMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugTimeMenuOpen>();
        app.init_resource::<DebugCharacterSelection>();
        app.init_resource::<DebugPerfSettings>();
        app.add_systems(
            Update,
            toggle_debug_time_menu.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            close_debug_time_menu_on_escape
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            handle_backdrop_click
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            sync_debug_character_selection.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            sync_debug_menu_open_state
                .run_if(in_state(GameState::Playing))
                .after(toggle_debug_time_menu)
                .after(crate::render::systems::apply_cursor_grab),
        );
        app.add_systems(
            Update,
            spawn_debug_time_menu
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_character_button_label
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_perf_button_labels
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            style_debug_time_menu
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            handle_debug_menu_interactions
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(Update, despawn_debug_time_menu.run_if(debug_menu_closed));
        app.add_systems(OnEnter(GameState::MainMenu), close_debug_menu_on_main_menu);
    }
}

#[derive(Resource, Default)]
pub struct DebugTimeMenuOpen(pub bool);

#[derive(Resource, Default)]
pub struct DebugCharacterSelection {
    pub current: PlayerCharacter,
}

#[derive(Resource, Default)]
pub struct DebugPerfSettings {
    pub weightmap_stats: bool,
    pub render_diag_logging: bool,
}

#[derive(Component)]
struct DebugMenuRoot;
#[derive(Component)]
struct DebugMenuBackdrop;
#[derive(Component)]
struct DebugMenuPanel;

#[derive(Component, Clone, Copy)]
struct TimeButton(TimeOfDayPreset);

#[derive(Component)]
struct CloseButton;

#[derive(Component)]
struct FlyToggleButton;

#[derive(Component, Clone, Copy)]
struct CloudCoverButton(CloudCoverMode);

#[derive(Component)]
struct CharacterToggleButton;

#[derive(Component)]
struct CharacterLabel;

#[derive(Component)]
struct PerfWeightmapToggleButton;

#[derive(Component)]
struct PerfWeightmapLabel;

#[derive(Component)]
struct PerfRenderDiagToggleButton;

#[derive(Component)]
struct PerfRenderDiagLabel;

#[derive(Component)]
struct SpawnOilmanNpcButton;
