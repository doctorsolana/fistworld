//! Persistent gameplay HUD (world clock, god-mode toggle, simulation speed).
//!
//! Unlike the modal panels this never sets an `InputState` flag — the camera must keep
//! panning underneath it.

pub mod actions;
pub mod layout;
pub mod state_sync;

use actions::{handle_mode_chip_button, handle_mode_toggle_key, handle_warp_buttons};
use layout::{despawn_hud, spawn_hud};
use state_sync::{
    receive_dev_status, reset_dev_grant, style_warp_buttons, sync_clock_chip, sync_god_panel,
    sync_mode_chip,
};

use bevy::prelude::*;
use lightyear::prelude::*;

use shared::components::{TimeWarp, WorldTime};
use shared::protocol::{DevCommand, DevStatus, ReliableChannel};

use crate::input::InputState;
use crate::states::GameState;
use crate::ui::styles::{
    ACCENT_COLOR, BUTTON_BORDER, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, TEXT_COLOR,
    TEXT_MUTED,
};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GodCapability>();
        app.init_resource::<HudMode>();
        // A fresh connection must not inherit a stale god grant.
        app.add_systems(OnEnter(GameState::Connecting), reset_dev_grant);
        app.add_systems(Update, receive_dev_status);
        app.add_systems(OnEnter(GameState::Playing), spawn_hud);
        app.add_systems(OnExit(GameState::Playing), despawn_hud);
        app.add_systems(
            Update,
            (
                handle_mode_toggle_key,
                handle_mode_chip_button,
                handle_warp_buttons,
                sync_clock_chip,
                sync_mode_chip,
                sync_god_panel,
                style_warp_buttons,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Whether the server granted this connection god capability (via [`DevStatus`]).
#[derive(Resource, Default)]
pub struct GodCapability(pub bool);

/// Which HUD mode the player is in. God is only reachable while [`GodCapability`] holds.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
pub enum HudMode {
    #[default]
    Play,
    God,
}

impl HudMode {
    pub(super) fn toggled(self) -> Self {
        match self {
            HudMode::Play => HudMode::God,
            HudMode::God => HudMode::Play,
        }
    }
}

pub(super) const PANEL_BACKGROUND: Color = Color::srgba(0.06, 0.05, 0.04, 0.88);
/// Dark text on the accent-filled active speed button.
pub(super) const WARP_ACTIVE_TEXT: Color = Color::srgb(0.08, 0.06, 0.04);

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct ClockPeriodText;

#[derive(Component)]
struct ClockTimeText;

#[derive(Component)]
struct ClockWarpText;

#[derive(Component)]
struct ModeChipButton;

#[derive(Component)]
struct ModeChipText;

#[derive(Component)]
struct GodPanel;

#[derive(Component, Clone, Copy)]
struct WarpButton(f32);
