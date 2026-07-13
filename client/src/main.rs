//! Game Client - Renders the world and handles player input.

mod app_wiring;
mod audio;
mod camera;
mod chest;
mod city;
mod crosshair;
mod dialogue;
mod input;
mod pickup;
mod profiling;
mod props;
mod rail;
mod render;
mod states;
mod streaming;
mod terrain;
mod ui;
mod water;
mod weapon_view;
mod weapons;

use bevy::prelude::*;
use shared::protocol::{SERVER_ADDR, SERVER_PORT};

/// Marker component for our client entity.
#[derive(Component)]
pub struct GameClient;

/// Get the asset path - for bundled macOS apps, use path relative to executable.
fn get_asset_path() -> String {
    // Try to find assets relative to executable (for .app bundles)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_assets = exe_dir.join("assets");
            if bundled_assets.exists() {
                info!("Using bundled assets at: {:?}", bundled_assets);
                return bundled_assets.to_string_lossy().to_string();
            }
        }
    }
    // Fall back to default "assets" folder (for development)
    "assets".to_string()
}

fn main() {
    let asset_path = get_asset_path();

    let mut app = App::new();
    app_wiring::setup_plugins(&mut app, asset_path);
    app_wiring::setup_resources(&mut app);
    app_wiring::setup_systems(&mut app);

    // Generate a unique client ID for logging
    let client_id = rand::random::<u64>();
    info!(
        "Starting client, server at {}:{}, client_id: {}",
        SERVER_ADDR, SERVER_PORT, client_id
    );

    app.run();
}
