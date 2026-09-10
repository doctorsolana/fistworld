//! Pause menu UI (in-game escape menu)
//!
//! Now includes graphics settings panel for troubleshooting flickering/performance.
//! Menu smoothly slides when opening/closing the graphics panel.

pub mod actions;
pub mod animation;
mod capture_fixture;
mod display;
mod display_capture;
pub mod layout;
mod sliders;
pub mod widgets;

use actions::{
    handle_display_confirmation, handle_escape_key, handle_graphics_toggles, handle_pause_actions,
    pause_menu_closed, pause_menu_open, reset_menu_state, sync_display_confirmation,
    sync_pause_menu_cursor,
};
use animation::animate_menu_transition;
use display::{handle_display_modes, handle_display_steps, sync_display_controls};
use layout::{despawn_pause_menu, spawn_pause_menu};
use sliders::{handle_input_slider_steps, handle_slider_steps, sync_slider_controls};
use widgets::{spawn_button, spawn_controls_panel, spawn_graphics_panel};

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::{CursorOptions, Monitor, PrimaryMonitor, PrimaryWindow};
use lightyear::prelude::client::*;

use super::foundation::{
    button_chrome, selected_button_chrome, UiButtonLabel, UiButtonStyle, UiButtonVariant,
};
use super::modal::{modal_root_chrome, sync_modal_cursor, ModalRoot};
use super::styles::*;
use crate::input::InputState;
use crate::render::systems::{
    available_display_resolutions, clamped_render_scale, scene_render_resolution, DisplayMode,
    DisplayResolution, GraphicsSettings, InputSettings, PendingDisplayChange,
};
use crate::states::GameState;
use crate::GameClient;

/// Deterministically opens a pause-menu state for the offline visual harness.
pub(crate) fn open_for_capture(commands: &mut Commands, panel: &str) {
    let (graphics_open, controls_open) = match panel {
        "graphics" | "graphics-confirmation" => (true, false),
        "controls" => (false, true),
        _ => (false, false),
    };
    commands.insert_resource(PauseMenuOpen(true));
    commands.insert_resource(PauseMenuState {
        graphics_open,
        controls_open,
        transition: if graphics_open || controls_open {
            1.0
        } else {
            0.0
        },
    });
}

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        display_capture::install(app);
        capture_fixture::install(app);
        app.init_resource::<PauseMenuState>();
        app.init_resource::<PauseMenuOpen>();
        app.add_systems(Update, sync_pause_menu_cursor.run_if(pause_menu_open));
        app.add_systems(Update, spawn_pause_menu.run_if(pause_menu_open));
        app.add_systems(
            Update,
            (despawn_pause_menu, reset_menu_state).run_if(pause_menu_closed),
        );
        app.add_systems(
            Update,
            (
                handle_pause_actions,
                handle_graphics_toggles,
                handle_slider_steps,
                handle_display_modes,
                handle_display_steps,
                handle_display_confirmation,
                sync_display_confirmation,
                handle_input_slider_steps,
                animate_menu_transition,
            )
                .run_if(pause_menu_open),
        );
        app.add_systems(
            PostUpdate,
            (sync_display_controls, sync_slider_controls)
                .before(super::button_motion::animate_buttons)
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
    Shadows,
    Atmosphere,
    Clouds,
    Vsync,
}

/// Slider controls (for view distance and prop distance)
#[derive(Component, Clone, Copy, Debug)]
enum SliderControl {
    Resolution,
    RenderScale,
    ShadowQuality,
    Exposure,
    ViewDistance,
    PropDistance,
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

/// Keep or roll back a newly applied output mode/resolution.
#[derive(Component, Clone, Copy, Debug)]
enum DisplayConfirmationAction {
    Keep,
    Revert,
}

#[derive(Component)]
struct DisplayConfirmationPanel;

#[derive(Component)]
struct DisplayConfirmationText;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_luminance(color: Color) -> f32 {
        let rgba = color.to_srgba();
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgba.red) + 0.7152 * linear(rgba.green) + 0.0722 * linear(rgba.blue)
    }

    fn contrast(first: Color, second: Color) -> f32 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn expanded_pause_panels_keep_readable_text_contrast() {
        assert!(contrast(INK_INVERSE, FRONT_PANEL) >= 7.0);
        assert!(contrast(INK_INVERSE_MUTED, FRONT_PANEL) >= 7.0);
        assert!(contrast(INK_INVERSE_HEADING, FRONT_PANEL) >= 7.0);
    }

    #[test]
    fn pending_display_change_opens_the_countdown_prompt() {
        let mut app = App::new();
        app.insert_resource(PendingDisplayChange::new(&GraphicsSettings::default()));
        app.add_systems(Update, sync_display_confirmation);
        let panel = app
            .world_mut()
            .spawn((
                DisplayConfirmationPanel,
                Node {
                    display: Display::None,
                    ..default()
                },
            ))
            .id();
        let label = app
            .world_mut()
            .spawn((
                DisplayConfirmationText,
                Text::new("Keep this display setting?"),
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
        assert!(app
            .world()
            .get::<Text>(label)
            .unwrap()
            .0
            .contains("Reverting in 15s"));
    }

    #[test]
    fn revert_button_restores_the_last_confirmed_display_setting() {
        let mut previous = GraphicsSettings::default();
        previous.set_display_mode(DisplayMode::Windowed);
        previous.display_resolution = DisplayResolution::new(1600, 900);
        let mut candidate = previous.clone();
        candidate.set_display_mode(DisplayMode::ExclusiveFullscreen);
        candidate.display_resolution = DisplayResolution::new(1920, 1200);

        let mut app = App::new();
        app.insert_resource(candidate);
        app.insert_resource(PendingDisplayChange::new(&previous));
        app.add_systems(Update, handle_display_confirmation);
        app.world_mut()
            .spawn((Interaction::Pressed, DisplayConfirmationAction::Revert));

        app.update();

        let restored = app.world().resource::<GraphicsSettings>();
        assert_eq!(restored.display_mode(), DisplayMode::Windowed);
        assert_eq!(
            restored.display_resolution,
            DisplayResolution::new(1600, 900)
        );
        assert!(!app.world().contains_resource::<PendingDisplayChange>());
    }
}
