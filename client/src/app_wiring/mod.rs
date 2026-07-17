//! Centralized app wiring helpers for plugins, resources, and systems.

pub mod dev;
pub mod plugins;
pub mod resources;
pub mod systems;
pub mod window;

pub use plugins::setup_plugins;
pub use resources::setup_resources;
pub use systems::setup_systems;

use window::apply_connect_window_settings;

use bevy::asset::AssetPlugin;
use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::diagnostic::{
    EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::settings::{RenderCreation, WgpuFeatures, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::ui::UiScale;
use bevy::window::{
    Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, WindowMode, WindowResolution,
};
use lightyear::prelude::client::ClientPlugins;
use shared::protocol::{tick_duration, ProtocolPlugin};
use shared::weapons::WeaponDebugMode;

use crate::{
    audio, camera, chest, city, crosshair, dialogue, input, pickup, profiling, props, rail, render,
    render::systems as game_systems, states::GameState, terrain, ui, water, weapon_view, weapons,
};
use game_systems::{GraphicsSettings, LAUNCHER_RESOLUTION};
