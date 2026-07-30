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
    /// True when the inventory UI is open.
    pub inventory_open: bool,
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
}

impl InputState {
    pub(crate) fn ui_blocking(&self) -> bool {
        self.inventory_open
            || self.pause_menu_open
            || self.map_open
            || self.debug_menu_open
            || self.hero_creator_open
            || self.encyclopedia_open
    }
}
