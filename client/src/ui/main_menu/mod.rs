//! Main menu UI
//!
//! Server connection launcher and preset dropdown.

pub mod actions;
pub mod dropdown;
pub mod layout;
pub mod network_input;

use actions::{animate_logo, handle_menu_actions};
use dropdown::{
    handle_dropdown_selection, handle_dropdown_toggle, spawn_dropdown, update_dropdown_display,
};
use layout::{apply_launcher_window_settings, despawn_main_menu, spawn_main_menu};
use network_input::{
    handle_ip_input_focus, handle_ip_keyboard_input, load_server_presets_sync, update_ip_display,
};

use arboard::Clipboard;
use bevy::app::AppExit;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow};
use serde::Deserialize;

use super::foundation::{
    button_chrome, selected_button_chrome, UiButtonLabel, UiButtonStyle, UiButtonVariant,
};
use super::styles::*;
use crate::render::systems::{DisplayMode, DisplayResolution, LAUNCHER_RESOLUTION};
use crate::states::GameState;
use shared::protocol::SERVER_PORT;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        // Load server presets synchronously during plugin build (before any systems run)
        let (presets, server_address) = load_server_presets_sync();
        app.insert_resource(presets);
        app.insert_resource(server_address);
        app.init_resource::<DropdownState>();

        app.add_systems(
            OnEnter(GameState::MainMenu),
            (apply_launcher_window_settings, spawn_main_menu),
        );
        app.add_systems(OnExit(GameState::MainMenu), despawn_main_menu);
        app.add_systems(
            Update,
            (
                handle_menu_actions,
                animate_logo,
                handle_ip_input_focus,
                handle_ip_keyboard_input,
                update_ip_display,
                handle_dropdown_toggle,
                handle_dropdown_selection,
                update_dropdown_display,
            )
                .run_if(in_state(GameState::MainMenu)),
        );
    }
}

// =============================================================================
// CONFIG TYPES
// =============================================================================

/// A single server entry from the config file
#[derive(Debug, Clone, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub ip: String,
}

/// The servers.ron config file structure
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub servers: Vec<ServerEntry>,
    pub default_index: usize,
}

/// Resource holding loaded server presets
#[derive(Resource, Default)]
pub struct ServerPresets {
    pub entries: Vec<ServerEntry>,
    pub selected_index: Option<usize>,
}

/// Resource holding the server address to connect to
#[derive(Resource)]
pub struct ServerAddress {
    pub ip: String,
    pub port: u16,
}

impl Default for ServerAddress {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: SERVER_PORT,
        }
    }
}

/// Dropdown open/closed state
#[derive(Resource, Default)]
pub struct DropdownState {
    pub expanded: bool,
}

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the main menu root
#[derive(Component)]
struct MainMenuRoot;

/// Marker for the logo (for animation)
#[derive(Component)]
struct LogoImage {
    base_scale: f32,
    time: f32,
}

/// Marker for the IP input field
#[derive(Component)]
struct IpInputField {
    focused: bool,
}

/// Marker for the IP text display
#[derive(Component)]
struct IpTextDisplay;

/// Button action types
#[derive(Component, Clone, Copy)]
enum MenuButton {
    Connect,
    Exit,
}

/// Dropdown toggle button
#[derive(Component)]
struct DropdownToggle;

/// Dropdown text display (shows selected preset name)
#[derive(Component)]
struct DropdownText;

/// Container for dropdown options (shown when expanded)
#[derive(Component)]
struct DropdownOptions;

/// A single dropdown option
#[derive(Component)]
struct DropdownOption {
    index: usize,
}

// =============================================================================
// STARTUP: LOAD CONFIG
// =============================================================================

// =============================================================================
// MENU SPAWN
// =============================================================================

// =============================================================================
// INTERACTIONS
// =============================================================================

// =============================================================================
// DROPDOWN LOGIC
// =============================================================================
