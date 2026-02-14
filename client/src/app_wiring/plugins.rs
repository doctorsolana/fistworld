//! plugins systems.

use super::*;

pub fn setup_plugins(app: &mut App, asset_path: String) {
    // Full Bevy with rendering - configure asset path for bundled apps
    // Performance: Disable MSAA (expensive), enable GPU-driven rendering
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Sandbox Game".to_string(),
                    resolution: WindowResolution::new(LAUNCHER_RESOLUTION.0, LAUNCHER_RESOLUTION.1),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_path,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic({
                    let mut settings = WgpuSettings {
                        // Enable GPU-driven rendering for better batching
                        features: WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                        ..default()
                    };
                    // Metal counter sample buffers can fail on macOS under heavy diagnostics usage.
                    // Disable GPU timestamp / pipeline stats queries to avoid device loss.
                    #[cfg(target_os = "macos")]
                    {
                        settings.disabled_features = Some(
                            WgpuFeatures::TIMESTAMP_QUERY
                                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_PASSES
                                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
                                | WgpuFeatures::PIPELINE_STATISTICS_QUERY,
                        );
                    }
                    settings
                }),
                ..default()
            })
            // Spatial audio in rodio uses strong inverse-square falloff. Our world units are ~meters
            // (player height ~1.8), so we scale distances down to make spatial audio audible.
            .set(AudioPlugin {
                default_spatial_scale: SpatialScale::new(0.2),
                ..default()
            }),
    );

    // FPS diagnostics for debug overlay
    app.add_plugins(FrameTimeDiagnosticsPlugin::default());
    // Extra diagnostics for the debug overlay (entity count + optional render pass timings)
    app.add_plugins(EntityCountDiagnosticsPlugin::default());
    let render_diag_enabled = std::env::var("FISTFORCE_RENDER_DIAG")
        .map(|v| v == "1")
        .unwrap_or(false);
    if render_diag_enabled {
        app.add_plugins(RenderDiagnosticsPlugin);
        info!("Render diagnostics plugin enabled via FISTFORCE_RENDER_DIAG=1");
    } else {
        info!("Render diagnostics plugin disabled (set FISTFORCE_RENDER_DIAG=1 to enable)");
    }
    app.add_plugins(SystemInformationDiagnosticsPlugin);

    // Game state machine
    app.init_state::<GameState>();

    // Lightyear client plugins (tick_duration = 60Hz)
    app.add_plugins(ClientPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);

    // Terrain generation and rendering
    app.add_plugins(terrain::TerrainPlugin);
    app.add_plugins(water::WaterPlugin);
    app.add_plugins(render::sniper_fisheye::SniperFisheyePlugin);

    // Environmental props (rocks, trees, etc.)
    app.add_plugins(props::PropsPlugin);

    // UI plugins
    app.add_plugins(ui::MainMenuPlugin);
    app.add_plugins(ui::PauseMenuPlugin);
    app.add_plugins(ui::InventoryPlugin);
    app.add_plugins(ui::NameEntryPlugin);
    app.add_plugins(ui::WorldMapPlugin);
    app.add_plugins(ui::DebugTimeMenuPlugin);

    // Pickup plugin (item pickups with E key)
    app.add_plugins(pickup::PickupPlugin);

    // Chest plugin (storage containers)
    app.add_plugins(chest::ChestPlugin);

    // Audio plugin
    app.add_plugins(audio::GameAudioPlugin);

    // NPC Dialogue plugin
    app.add_plugins(dialogue::DialoguePlugin);
}
