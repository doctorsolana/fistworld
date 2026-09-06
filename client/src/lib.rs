//! Game client library.
//!
//! `main.rs` is a thin wrapper around [`run`]; the modules live here so other binaries
//! in this crate (notably `src/bin/capture.rs`) can reuse the rendering stack.

pub mod app_wiring;
pub mod army_roster;
pub mod audio;
pub mod battalion_bar;
pub mod boat;
pub mod camera_rts;
pub mod capture;
pub mod capture_artifact;
pub mod city;
pub mod combat_mode;
pub mod hero;
pub mod input;
pub mod perf_overlay;
pub mod profiling;
pub mod props;
pub mod render;
pub mod selection;
pub mod settlement;
pub mod standard_flag;
pub mod states;
pub mod streaming;
pub mod terrain;
pub mod ui;
pub mod water;
pub mod wind;

use bevy::prelude::*;
use shared::protocol::{SERVER_ADDR, SERVER_PORT};

/// Marker component for our client entity.
#[derive(Component)]
pub struct GameClient;

/// Get the asset path - for bundled macOS apps, use path relative to executable.
pub fn get_asset_path() -> String {
    // Development and capture harnesses can explicitly select the source asset
    // tree. This also avoids accidentally preferring a stale partial bundle
    // beside an optimized executable.
    if let Ok(asset_root) = std::env::var("BEVY_ASSET_ROOT") {
        let asset_root = asset_root.trim();
        if !asset_root.is_empty() {
            info!("Using assets from BEVY_ASSET_ROOT: {asset_root}");
            return asset_root.to_owned();
        }
    }

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

/// Boot the normal multiplayer client.
pub fn run() -> AppExit {
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

    app.run()
}

pub mod siege;
