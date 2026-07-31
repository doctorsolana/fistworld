//! Persistent gameplay HUD (world clock, god-mode toggle, simulation speed).
//!
//! Unlike the modal panels this never sets an `InputState` flag — the camera must keep
//! panning underneath it.

pub mod actions;
pub mod layout;
pub mod state_sync;

use actions::{
    handle_mode_chip_button, handle_mode_toggle_key, handle_spawn_hero_button,
    handle_warp_buttons,
};
use layout::{despawn_hud, spawn_hud};
use state_sync::{
    receive_dev_status, reset_dev_grant, style_warp_buttons, sync_clock_chip,
    sync_god_panel, sync_mode_chip, sync_spawn_hero_button,
};

use bevy::prelude::*;
use lightyear::prelude::*;

use shared::components::{TimeWarp, WorldTime};
use shared::protocol::{DevCommand, DevStatus, ReliableChannel};

use crate::input::InputState;
use crate::states::GameState;
use crate::ui::styles::{
    plate_shadow, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, EMBER, EMBER_RULE, INK,
    INK_INVERSE, INK_MUTED, LIMEWASH, LIMEWASH_LIT, PLATE_RULE, PLATE_RULE_SOFT, RADIUS, SLATE,
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
                handle_spawn_hero_button,
                actions::handle_spawn_npc_button,
                actions::handle_found_village_button,
                sync_clock_chip,
                sync_mode_chip,
                sync_god_panel,
                style_warp_buttons,
                sync_spawn_hero_button,
                state_sync::sync_spawn_npc_button,
                state_sync::sync_found_village_button,
                state_sync::sync_selection_plate,
                state_sync::sync_selection_box,
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

/// Every HUD plate.
pub const PANEL_BACKGROUND: Color = LIMEWASH;
/// Text ON the filled active speed button. Must be the light inverse: ink on a
/// dark slate fill is unreadable, and this is the one place the value flips.
pub(super) const WARP_ACTIVE_TEXT: Color = INK_INVERSE;

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

#[derive(Component)]
struct SpawnHeroButton;

#[derive(Component)]
struct SpawnHeroLabel;

#[derive(Component)]
struct SpawnNpcButton;

#[derive(Component)]
struct SpawnNpcLabel;

#[derive(Component)]
struct FoundVillageButton;

#[derive(Component)]
struct FoundVillageLabel;

// --- the selected-unit plate ------------------------------------------------

/// Root of the bottom-centre plate. Present always, shown only when something
/// is selected, so appearing costs no spawn.
#[derive(Component)]
struct SelectionPlate;

/// The drag-select marquee.
#[derive(Component)]
struct SelectionBox;

/// The hollow ring mark that rhymes with the ring on the ground.
#[derive(Component)]
struct SelectionRingGlyph;

#[derive(Component)]
struct SelectionNameText;

#[derive(Component)]
struct SelectionStatusText;
