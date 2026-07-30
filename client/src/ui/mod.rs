//! UI module

use bevy::prelude::*;

/// Put this on any UI node that must swallow world clicks.
///
/// The world-click guards used to test an UNFILTERED `Query<&Interaction>`:
/// "is any UI element in the whole app hovered or pressed?". That is a veto that
/// grows every time a panel is added, and it breaks badly for a wide node --
/// one full-width bar hovering anywhere would make the entire world
/// unclickable, with no error and no obvious cause.
///
/// Opting in per node instead means the guard's cost and blast radius are both
/// explicit: the HUD plates and the selection plate block, and a decorative
/// full-width container does not.
#[derive(Component, Default)]
pub struct BlocksWorldClicks;

/// True when the cursor is over a UI surface that owns clicks.
pub fn pointer_over_ui(blockers: &Query<&Interaction, With<BlocksWorldClicks>>) -> bool {
    blockers
        .iter()
        .any(|interaction| *interaction != Interaction::None)
}

pub mod debug_time_menu;
pub mod encyclopedia;
pub mod hero_creator;
pub mod hud;
pub mod main_menu;
pub mod modal;
pub mod name_entry;
pub mod pause_menu;
pub mod styles;
pub mod world_map;

pub use debug_time_menu::{DebugPerfSettings, DebugTimeMenuPlugin};
pub use encyclopedia::EncyclopediaPlugin;
pub use hero_creator::HeroCreatorPlugin;
pub use hud::HudPlugin;
pub use main_menu::MainMenuPlugin;
pub use main_menu::ServerAddress;
pub use name_entry::NameEntryPlugin;
pub use pause_menu::PauseMenuPlugin;
pub use world_map::WorldMapPlugin;
