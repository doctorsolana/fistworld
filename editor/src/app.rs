use std::path::PathBuf;

use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

use shared::map::DEFAULT_MAP_ID;

use crate::camera;
use crate::city;
use crate::lighting;
use crate::picking;
use crate::session::{
    BrushStroke, CursorTerrainHit, EditorEnvironmentState, EditorUiState, PropPreviewState,
    TerrainChunkRegistry, UiActionRequests, WaterChunkRegistry,
};
use crate::terrain_material::{
    configure_editor_terrain_arrays, EditorTerrainSplatMaterial, EditorTerrainTextureAssets,
};
use crate::tools::{self, VisualRefreshFlags};
use crate::ui;

pub fn run() {
    let map_id = parse_map_arg();
    std::env::set_var("CITYSIM_MAP_ID", &map_id);
    let asset_path = resolve_asset_path();
    std::env::set_var("FISTFORCE_ASSET_PATH", &asset_path);

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("Fist Engine Map Editor [{}]", map_id),
                    resolution: WindowResolution::new(1920, 1080),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                // Close requests go through the unsaved-changes guard
                // (tools::handle_window_close_requested) instead of
                // closing immediately.
                close_when_requested: false,
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_path,
                ..default()
            }),
    );
    app.add_plugins(EguiPlugin::default());
    app.add_plugins(MaterialPlugin::<EditorTerrainSplatMaterial>::default());

    app.init_resource::<EditorUiState>();
    app.init_resource::<CursorTerrainHit>();
    app.init_resource::<UiActionRequests>();
    app.init_resource::<BrushStroke>();
    app.init_resource::<PropPreviewState>();
    app.init_resource::<TerrainChunkRegistry>();
    app.init_resource::<WaterChunkRegistry>();
    app.init_resource::<VisualRefreshFlags>();
    app.init_resource::<city::CityEditorState>();
    app.init_resource::<EditorEnvironmentState>();
    app.init_resource::<ui::EditorPropCatalog>();
    app.init_resource::<EditorTerrainTextureAssets>();

    app.add_systems(
        Startup,
        (
            ui::build_prop_catalog,
            camera::spawn_editor_camera,
            tools::setup_editor_scene,
            city::setup_city_scene,
        )
            .chain(),
    );
    app.add_systems(EguiPrimaryContextPass, ui::editor_ui_panel);
    app.add_systems(
        Update,
        (
            camera::update_editor_camera,
            picking::update_cursor_terrain_hit,
            tools::handle_window_close_requested,
            tools::handle_editor_shortcuts,
            city::handle_city_shortcuts,
            city::handle_city_tool_input,
            city::handle_city_ui_actions,
            tools::handle_tool_input,
            configure_editor_terrain_arrays,
            tools::apply_visual_refresh,
            tools::cull_distant_prop_visuals,
            city::apply_city_visual_refresh,
            lighting::apply_editor_environment,
            tools::sync_editor_terrain_water,
            tools::refresh_cursor_indicator,
            tools::update_prop_preview_visual,
            city::update_city_preview_visuals,
        )
            .chain(),
    );

    app.run();
}

fn parse_map_arg() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--map=") {
            if !value.trim().is_empty() {
                return value.trim().to_string();
            }
        }
        if arg == "--map" {
            if let Some(value) = args.next() {
                if !value.trim().is_empty() {
                    return value.trim().to_string();
                }
            }
        }
    }
    DEFAULT_MAP_ID.to_string()
}

fn resolve_asset_path() -> String {
    if let Ok(path) = std::env::var("FISTFORCE_ASSET_PATH") {
        if !path.trim().is_empty() {
            return path;
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent() {
        let shared_assets = workspace_root.join("client/assets");
        if shared_assets.exists() {
            return shared_assets.to_string_lossy().to_string();
        }
    }

    let local_client_assets = PathBuf::from("client/assets");
    if local_client_assets.exists() {
        return local_client_assets.to_string_lossy().to_string();
    }

    "client/assets".to_string()
}
