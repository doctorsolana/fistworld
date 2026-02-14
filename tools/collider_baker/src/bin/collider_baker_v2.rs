//! Offline collider baking tool (v2).
//!
//! Reads `client/assets/colliders_manifest.ron`, loads referenced GLTF scenes via Bevy,
//! computes convex hull or convex decomposition colliders, and writes
//! `client/assets/colliders.bin` for runtime use.

use bevy::prelude::*;

#[path = "collider_baker_v2/filters.rs"]
mod filters;
#[path = "collider_baker_v2/pipeline.rs"]
mod pipeline;
#[path = "collider_baker_v2/scene.rs"]
mod scene;
#[path = "collider_baker_v2/types.rs"]
mod types;
#[path = "collider_baker_v2/vhacd.rs"]
mod vhacd;

use pipeline::{poll_and_bake, start_bake};
use types::BakeConfig;

fn main() {
    let workspace_root = std::env::current_dir().expect("cwd");
    let assets_dir = workspace_root.join("client/assets");
    let manifest_path = assets_dir.join("colliders_manifest.ron");
    let output_path = assets_dir.join("colliders.bin");

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets_dir.to_string_lossy().to_string(),
                ..default()
            }),
    );

    app.insert_resource(BakeConfig {
        manifest_path,
        output_path,
    });

    app.add_systems(Startup, start_bake);
    app.add_systems(Update, poll_and_bake);

    app.run();
}
