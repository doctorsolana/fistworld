//! plugins systems.

use super::*;

pub fn setup_plugins(app: &mut App, asset_path: String) {
    let hitch_profile_enabled = profiling::hitch_profiling_enabled();

    // Full Bevy with rendering - configure asset path for bundled apps
    // Performance: Disable MSAA (expensive), enable GPU-driven rendering
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "FistForce".to_string(),
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
                    // Bevy 0.19's GPU diagnostic recorder supports Vulkan/DX12,
                    // not Metal. RenderDiagnosticsPlugin still reports CPU
                    // timings here; enabling counter sample buffers provides
                    // no GPU timings and can fail under heavy diagnostics.
                    #[cfg(target_os = "macos")]
                    {
                        settings.disabled_features = Some(
                            WgpuFeatures::TIMESTAMP_QUERY
                                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_PASSES
                                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
                                | WgpuFeatures::PIPELINE_STATISTICS_QUERY,
                        );
                    }
                    Box::new(settings)
                }),
                // Capture scenarios prefer reproducibility to startup speed.
                // On platforms where Bevy supports it, this prevents a first
                // screenshot from racing asynchronous pipeline compilation.
                synchronous_pipeline_compilation: profiling::env_flag(
                    "FISTFORCE_CAPTURE_SYNC_PIPELINES",
                ),
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
    let render_diag_enabled = profiling::env_flag("FISTFORCE_RENDER_DIAG");
    if render_diag_enabled {
        app.add_plugins(RenderDiagnosticsPlugin);
        info!("Render diagnostics plugin enabled");
    } else {
        info!("Render diagnostics plugin disabled (set FISTFORCE_RENDER_DIAG=1 to enable)");
    }
    // System info sampling (process CPU/memory via sysinfo) polls the OS on a
    // schedule; keep it opt-in rather than a permanent background cost.
    if profiling::env_flag("FISTFORCE_SYSINFO_DIAG") {
        app.add_plugins(SystemInformationDiagnosticsPlugin);
        info!("System information diagnostics enabled");
    }
    if profiling::env_flag("FISTFORCE_LOG_DIAGNOSTICS") {
        // With render diagnostics on, log everything — the render/*/elapsed_cpu
        // paths are the whole point of a headless profiling run.
        let filter = if render_diag_enabled {
            None
        } else {
            Some(
                [
                    FrameTimeDiagnosticsPlugin::FRAME_TIME,
                    FrameTimeDiagnosticsPlugin::FPS,
                    EntityCountDiagnosticsPlugin::ENTITY_COUNT,
                    SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE,
                    SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE,
                ]
                .into_iter()
                .collect(),
            )
        };
        app.add_plugins(LogDiagnosticsPlugin {
            wait_duration: std::time::Duration::from_secs(2),
            filter,
            ..default()
        });
        info!("Bevy log diagnostics enabled");
    }
    if hitch_profile_enabled {
        info!(
            "Hitch profiling enabled: walking through busy areas will log ClientPerf and Hitch snapshots for frames over threshold. Set FISTFORCE_RENDER_DIAG=1 for render diagnostics or FISTFORCE_LOG_DIAGNOSTICS=1 for periodic Bevy diagnostics."
        );
    }

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
    app.add_plugins(bevy::pbr::MaterialPlugin::<game_systems::CloudLayerMaterial>::default());
    app.add_plugins(city::CityPlugin);

    // Environmental props (rocks, trees, etc.)
    app.add_plugins(props::PropsPlugin);

    // UI plugins
    app.add_plugins(ui::UiFoundationPlugin);
    app.add_plugins(ui::UiScrollPlugin);
    app.add_plugins(ui::UiPerfPlugin);
    app.add_plugins(ui::MainMenuPlugin);
    app.add_plugins(ui::PauseMenuPlugin);
    app.add_plugins(ui::SettlementPanelPlugin);
    app.add_plugins(ui::MarketPlugin);
    app.add_plugins(ui::BusinessManagementPlugin);
    app.add_plugins(ui::PropertyMarketPlugin);
    app.add_plugins(ui::PlayerPermitsPlugin);
    app.add_plugins(ui::NameEntryPlugin);
    app.add_plugins(ui::DebugTimeMenuPlugin);
    app.add_plugins(ui::HudPlugin);
    app.add_plugins(crate::hero::HeroPlugin);
    app.add_plugins(crate::animals::AnimalsPlugin);
    app.add_plugins(crate::boat::BoatPlugin);
    app.add_plugins(crate::siege::SiegePlugin);
    app.add_plugins(crate::selection::SelectionPlugin);
    app.add_plugins(crate::settlement::SettlementPlugin);
    app.add_plugins(ui::settlement_names::SettlementNamesPlugin);
    app.add_plugins(crate::combat_mode::CombatModePlugin);
    app.add_plugins(crate::battalion_bar::BattalionBarPlugin);
    app.add_plugins(crate::standard_flag::StandardFlagPlugin);
    app.add_plugins(ui::HeroCreatorPlugin);
    app.add_plugins(ui::EncyclopediaPlugin);
    app.add_plugins(ui::CompanyFoundingPlugin);
    app.add_plugins(ui::HistoryPlugin);

    app.add_plugins(ui::WorldMapPlugin);
    app.add_plugins(audio::GameAudioPlugin);
}
