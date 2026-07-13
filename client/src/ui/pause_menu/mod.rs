//! Pause menu UI (in-game escape menu)
//!
//! Updated for Bevy 0.18 / Lightyear 0.26
//! Now includes graphics settings panel for troubleshooting flickering/performance.
//! Menu smoothly slides when opening/closing the graphics panel.

pub mod actions;
pub mod animation;
pub mod layout;
pub mod widgets;

use actions::{
    button_interactions, handle_escape_key, handle_graphics_toggles, handle_input_slider_steps,
    handle_pause_actions, handle_slider_steps, pause_menu_closed, pause_menu_open,
    reset_menu_state, sync_pause_menu_cursor,
};
use animation::animate_menu_transition;
use layout::{despawn_pause_menu, spawn_pause_menu};
use widgets::{spawn_button, spawn_controls_panel, spawn_graphics_panel, tonemapping_label};

use bevy::app::AppExit;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use lightyear::prelude::client::*;

use super::modal::sync_modal_cursor;
use super::styles::*;
use crate::input::InputState;
use crate::render::systems::{GraphicsSettings, InputSettings};
use crate::states::GameState;
use crate::GameClient;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenuState>();
        app.init_resource::<PauseMenuOpen>();
        app.add_systems(
            Update,
            sync_pause_menu_cursor
                .run_if(pause_menu_open)
                .after(crate::render::systems::apply_cursor_grab),
        );
        app.add_systems(Update, spawn_pause_menu.run_if(pause_menu_open));
        app.add_systems(
            Update,
            (despawn_pause_menu, reset_menu_state).run_if(pause_menu_closed),
        );
        app.add_systems(
            Update,
            (
                button_interactions,
                handle_pause_actions,
                handle_graphics_toggles,
                handle_slider_steps,
                handle_input_slider_steps,
                animate_menu_transition,
            )
                .run_if(pause_menu_open),
        );
        app.add_systems(
            Update,
            handle_escape_key.run_if(in_state(GameState::Playing)),
        );
    }
}

/// Tracks the pause menu animation state
#[derive(Resource, Default)]
struct PauseMenuState {
    graphics_open: bool,
    controls_open: bool,
    /// Animation progress: 0.0 = closed (centered), 1.0 = open (shifted left)
    transition: f32,
}

/// Tracks if the pause menu is open (without pausing the game).
#[derive(Resource, Default)]
struct PauseMenuOpen(pub bool);

/// Marker for the pause menu root
#[derive(Component)]
struct PauseMenuRoot;

/// Marker for the content container that gets shifted
#[derive(Component)]
struct MenuContentContainer;

/// Marker for the main menu column (buttons)
#[derive(Component)]
struct MainMenuColumn;

/// Marker for the graphics settings panel
#[derive(Component)]
struct GraphicsSettingsPanel;

/// Marker for the controls settings panel
#[derive(Component)]
struct ControlsSettingsPanel;

/// Pause menu button actions
#[derive(Component, Clone, Copy)]
enum PauseButton {
    Resume,
    Graphics,
    Controls,
    Disconnect,
    Exit,
}

/// Graphics toggle buttons
#[derive(Component, Clone, Copy, Debug)]
enum GraphicsToggle {
    Bloom,
    Ssao,
    Shadows,
    Atmosphere,
    Clouds,
    FarTerrain,
    Props,
    Vsync,
    Fullscreen,
    FoliageCutout,
}

/// Slider controls (for view distance and prop distance)
#[derive(Component, Clone, Copy, Debug)]
enum SliderControl {
    RenderScale,
    ShadowQuality,
    Tonemapping,
    Exposure,
    ViewDistance,
    PropDistance,
    LightingBoost,
}

/// Marker for toggle button text (so we can update it)
#[derive(Component)]
struct ToggleText(GraphicsToggle);

/// Marker for slider value text (so we can update it)
#[derive(Component)]
struct SliderValueText(SliderControl);

/// Step direction for slider buttons
#[derive(Component, Clone, Copy)]
struct SliderStep {
    control: SliderControl,
    delta: i32, // +1 or -1
}

/// Input/controls slider types
#[derive(Component, Clone, Copy, Debug)]
enum InputSliderControl {
    MouseSensitivity,
}

/// Marker for input slider value text
#[derive(Component)]
struct InputSliderValueText(InputSliderControl);

/// Step direction for input slider buttons
#[derive(Component, Clone, Copy)]
struct InputSliderStep {
    control: InputSliderControl,
    delta: i32, // +1 or -1
}
