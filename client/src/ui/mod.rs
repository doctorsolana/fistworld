//! UI module

pub mod debug_time_menu;
pub mod main_menu;
pub mod modal;
pub mod name_entry;
pub mod pause_menu;
pub mod styles;
pub mod world_map;

pub use debug_time_menu::{DebugPerfSettings, DebugTimeMenuPlugin};
pub use main_menu::MainMenuPlugin;
pub use main_menu::ServerAddress;
pub use name_entry::NameEntryPlugin;
pub use pause_menu::PauseMenuPlugin;
pub use world_map::WorldMapPlugin;
