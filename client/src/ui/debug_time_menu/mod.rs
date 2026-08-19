//! Debug time-of-day menu (client-side)

pub mod actions;
pub mod layout;
pub mod state_sync;

use actions::{
    close_debug_menu_on_main_menu, close_debug_time_menu_on_escape, debug_menu_closed,
    debug_menu_open, handle_backdrop_click, handle_debug_menu_interactions,
    handle_god_access_input, receive_god_access_result, toggle_debug_time_menu,
};
use layout::{despawn_debug_time_menu, spawn_debug_time_menu};
use state_sync::{
    sync_debug_menu_open_state, update_cloud_button_styles, update_perf_button_labels,
};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use lightyear::prelude::*;

use shared::protocol::{
    GodAccessResult, ReliableChannel, RequestGodAccess, SetTimeOfDay, TimeOfDayPreset,
};

use crate::input::InputState;
use crate::render::systems::{CloudCover, CloudCoverMode, CloudCoverOverride};
use crate::states::GameState;
use crate::ui::foundation::{
    button_chrome, selected_button_chrome, type_scale, UiButtonLabel, UiButtonStyle,
    UiButtonVariant,
};
use crate::ui::hud::{GodCapability, HudMode};
use crate::ui::modal::{handle_backdrop_pressed, spawn_modal, sync_modal_cursor, ModalLayout};
use crate::ui::styles::{
    plate_shadow, EMBER, INK, INK_INVERSE, INK_MUTED, LIMEWASH_HEADER, LIMEWASH_LIT, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS, SLATE,
};

pub struct DebugTimeMenuPlugin;

impl Plugin for DebugTimeMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugTimeMenuOpen>();
        app.init_resource::<DebugPerfSettings>();
        app.init_resource::<GodAccessInput>();
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
            sync_debug_menu_open_state
                .run_if(in_state(GameState::Playing))
                .after(toggle_debug_time_menu),
        );
        app.add_systems(
            Update,
            spawn_debug_time_menu
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (update_perf_button_labels, update_cloud_button_styles)
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            handle_debug_menu_interactions
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            handle_god_access_input
                .run_if(debug_menu_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            receive_god_access_result.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(Update, despawn_debug_time_menu.run_if(debug_menu_closed));
        app.add_systems(OnEnter(GameState::MainMenu), close_debug_menu_on_main_menu);
    }
}

#[derive(Resource, Default)]
pub struct DebugTimeMenuOpen(pub bool);

/// Ephemeral hosted-admin challenge state. The key is cleared on success,
/// close, and return to the main menu; it is never written to settings.
#[derive(Resource, Default)]
pub struct GodAccessInput {
    key: String,
    feedback: String,
    submitted: bool,
    skip_text_frame: bool,
}

#[derive(Resource)]
pub struct DebugPerfSettings {
    pub weightmap_stats: bool,
    pub render_diag_logging: bool,
}

impl Default for DebugPerfSettings {
    fn default() -> Self {
        let hitch_profile_enabled = crate::profiling::hitch_profiling_enabled();
        Self {
            weightmap_stats: crate::profiling::env_flag("FISTFORCE_WEIGHTMAP_STATS"),
            render_diag_logging: hitch_profile_enabled
                || crate::profiling::env_flag("FISTFORCE_HITCH_LOGGING")
                || crate::profiling::env_flag("FISTFORCE_RENDER_DIAG_LOGGING"),
        }
    }
}

#[derive(Component)]
struct DebugMenuRoot;
#[derive(Component)]
struct DebugMenuBackdrop;
#[derive(Component)]
struct DebugMenuPanel;

#[derive(Component)]
struct DebugMenuViewport;

#[derive(Component, Clone, Copy)]
struct TimeButton(TimeOfDayPreset);

#[derive(Component)]
struct CloseButton;

#[derive(Component, Clone, Copy)]
struct CloudCoverButton(CloudCoverMode);

#[derive(Component)]
struct PerfWeightmapToggleButton;

#[derive(Component)]
struct PerfWeightmapLabel;

#[derive(Component)]
struct PerfRenderDiagToggleButton;

#[derive(Component)]
struct PerfRenderDiagLabel;

#[derive(Component)]
struct GodAccessInputDisplay;

#[derive(Component)]
struct GodAccessFeedbackText;

#[derive(Component)]
struct GodAccessSubmitButton;
