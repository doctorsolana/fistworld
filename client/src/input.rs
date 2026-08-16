//! UI modal state.
//!
//! The FPS movement/look input pipeline died with the player body — camera control now
//! lives in [`crate::camera_rts`]. What survives is the modal mutex: several KEEP-list UI
//! panels set their own flag here, and anything that reacts to input checks
//! [`InputState::ui_blocking`] so the commander camera does not pan while a menu is open.

use bevy::prelude::*;

/// Which UI surfaces are currently capturing input.
#[derive(Resource, Default)]
pub struct InputState {
    /// True while any surface created by the shared modal foundation exists.
    pub modal_open: bool,
    /// True when pause menu is open.
    pub pause_menu_open: bool,
    /// True when world map is open.
    pub map_open: bool,
    /// True when debug time menu is open.
    pub debug_menu_open: bool,
    /// True when the hero creator modal is open.
    pub hero_creator_open: bool,
    /// True when the encyclopedia window is open.
    pub encyclopedia_open: bool,
    /// True while an on-demand settlement/market history ledger is open.
    pub history_open: bool,
    /// True while the owner is editing one business's operating policies.
    pub business_management_open: bool,
    /// True while the compact permit tray is open. It is not a full-screen
    /// modal, but it still owns pointer and keyboard input.
    pub permit_tray_open: bool,
}

impl InputState {
    pub(crate) fn ui_blocking(&self) -> bool {
        self.modal_open
            || self.pause_menu_open
            || self.map_open
            || self.debug_menu_open
            || self.hero_creator_open
            || self.encyclopedia_open
            || self.history_open
            || self.business_management_open
            || self.permit_tray_open
    }
}
