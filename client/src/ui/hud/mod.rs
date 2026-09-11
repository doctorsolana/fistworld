//! Persistent gameplay HUD (journey guidance, world clock and developer controls).
//!
//! Unlike the modal panels this never sets an `InputState` flag — the camera must keep
//! panning underneath it.

pub mod actions;
mod journey;
pub mod layout;
pub mod state_sync;

use actions::{
    handle_mode_chip_button, handle_mode_toggle_key, handle_spawn_hero_button, handle_warp_buttons,
};
use layout::{despawn_hud, spawn_hud};
use state_sync::{
    receive_dev_status, reset_dev_grant, style_warp_buttons, sync_clock_chip, sync_god_panel,
    sync_mode_chip, sync_spawn_hero_button,
};

use bevy::prelude::*;
use lightyear::prelude::*;

use shared::components::{TimeWarp, WorldTime};
use shared::protocol::{DevCommand, DevStatus, ReliableChannel};

use crate::input::InputState;
use crate::states::GameState;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonStyle, UiButtonVariant};
use crate::ui::styles::{
    plate_shadow, EMBER, EMBER_RULE, INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS,
};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        journey::install(app);
        app.init_resource::<GodCapability>();
        app.init_resource::<HudMode>();
        app.init_resource::<GodNotice>();
        app.init_resource::<ImmigrantBoatWatch>();
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
                (
                    actions::handle_spawn_npc_button,
                    actions::handle_spawn_catapult_button,
                ),
                actions::handle_immigrant_boat_button,
                actions::handle_found_village_button,
                actions::handle_selection_expand_button,
                sync_clock_chip,
                sync_mode_chip,
                sync_god_panel,
                style_warp_buttons,
                sync_spawn_hero_button,
                (
                    state_sync::sync_spawn_npc_button,
                    state_sync::sync_spawn_catapult_button,
                ),
                state_sync::sync_immigrant_boat_button,
                state_sync::sync_found_village_button,
                actions::watch_immigrant_boat.after(crate::camera_rts::update_commander_camera),
                tick_god_notice,
                // AFTER the gesture pipeline, or these draw last frame's
                // box/selection on whatever frames the scheduler reorders -
                // which the hand feels as intermittent lag.
                state_sync::sync_selection_plate.after(crate::selection::SelectionGestureSet),
                state_sync::sync_selection_box.after(crate::selection::SelectionGestureSet),
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// A short action result, retained by the notice tray and shown in developer controls.
///
/// Exists because a refused placement used to be indistinguishable from a
/// broken button — the click was consumed, the arm reset, and the reason lived
/// only in a server log.
#[derive(Resource, Default)]
pub struct GodNotice {
    pub text: String,
    pub seconds_left: f32,
    sequence: u64,
}

/// Village Lab camera state for an explicitly requested physical immigrant.
/// Existing boats are remembered so a click follows the boat it created, not
/// an older voyage that happens to still be offshore.
#[derive(Resource, Default)]
pub(super) struct ImmigrantBoatWatch {
    waiting: bool,
    following: Option<Entity>,
    known: bevy::platform::collections::HashSet<Entity>,
    waited_seconds: f32,
}

impl ImmigrantBoatWatch {
    fn active(&self) -> bool {
        self.waiting || self.following.is_some()
    }

    fn clear(&mut self) {
        self.waiting = false;
        self.following = None;
        self.waited_seconds = 0.0;
    }
}

impl GodNotice {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.seconds_left = 4.5;
        self.sequence = self.sequence.wrapping_add(1);
    }
}

#[derive(Component)]
struct GodNoticeText;

/// Fade the notice out, and keep the label in step with it.
fn tick_god_notice(
    time: Res<Time>,
    mut notice: ResMut<GodNotice>,
    mut labels: Query<(&mut Text, &mut Node), With<GodNoticeText>>,
) {
    if notice.seconds_left > 0.0 {
        notice.seconds_left -= time.delta_secs();
    }
    let showing = notice.seconds_left > 0.0;
    for (mut text, mut node) in labels.iter_mut() {
        let wanted = if showing {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
        if showing && text.0 != notice.text {
            text.0 = notice.text.clone();
        }
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
struct SpawnImmigrantBoatButton;

#[derive(Component)]
struct SpawnImmigrantBoatLabel;

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

#[derive(Component)]
struct SelectionHealthTrack;

#[derive(Component)]
struct SelectionHealthFill;

#[derive(Component)]
struct SelectionExpandButton;

#[derive(Component)]
struct SpawnCatapultButton;
#[derive(Component)]
struct SpawnCatapultLabel;
