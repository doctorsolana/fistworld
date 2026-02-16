use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

use shared::map::DEFAULT_MAP_ID;

use crate::camera;
use crate::picking;
use crate::session::{
    CursorTerrainHit, EditorEnvironmentState, EditorUiState, PropPreviewState,
    TerrainChunkRegistry, UiActionRequests, WaterChunkRegistry,
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
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_path,
                ..default()
            }),
    );
    app.add_plugins(EguiPlugin::default());

    app.init_resource::<EditorUiState>();
    app.init_resource::<CursorTerrainHit>();
    app.init_resource::<UiActionRequests>();
    app.init_resource::<PropPreviewState>();
    app.init_resource::<TerrainChunkRegistry>();
    app.init_resource::<WaterChunkRegistry>();
    app.init_resource::<VisualRefreshFlags>();
    app.init_resource::<EditorEnvironmentState>();
    app.init_resource::<ui::EditorPropCatalog>();

    app.add_systems(
        Startup,
        (
            ui::build_prop_catalog,
            camera::spawn_editor_camera,
            tools::setup_editor_scene,
        )
            .chain(),
    );
    app.add_systems(EguiPrimaryContextPass, ui::editor_ui_panel);
    app.add_systems(
        Update,
        (
            camera::update_editor_camera,
            picking::update_cursor_terrain_hit,
            tools::handle_editor_shortcuts,
            tools::handle_tool_input,
            tools::apply_visual_refresh,
            apply_editor_environment,
            tools::refresh_cursor_indicator,
            tools::update_prop_preview_visual,
        )
            .chain(),
    );

    app.run();
}

fn apply_editor_environment(
    mut env_state: ResMut<EditorEnvironmentState>,
    mut query_light: Query<(&mut DirectionalLight, &mut Transform)>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let Ok((mut light, mut light_transform)) = query_light.single_mut() else {
        return;
    };

    let clamped_day = env_state.day_time_hours.clamp(0.0, 24.0);
    if (clamped_day - env_state.day_time_hours).abs() > f32::EPSILON {
        env_state.day_time_hours = clamped_day;
    }
    let day_cycle = clamped_day / 24.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
    let sunlight = day_cycle.sin().clamp(-1.0, 1.0);
    let day_scale = (sunlight + 1.0) * 0.5;
    let night_scale = 1.0 - day_scale;
    let tilt = std::f32::consts::PI * 0.35 + sunlight * std::f32::consts::PI * 0.27;

    light.illuminance = 2_000.0 + day_scale * 48_000.0 + night_scale * 900.0;
    light.color = if sunlight > 0.0 {
        Color::srgb(1.0, 0.95, 0.85)
    } else {
        Color::srgb(0.22, 0.24, 0.33)
    };
    light.shadows_enabled = sunlight > -0.1;

    *light_transform =
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.2 + tilt, 0.8, 0.0));
    ambient.brightness = 250.0 + day_scale * 700.0;
    ambient.color = if sunlight > 0.0 {
        Color::srgb(0.9, 0.92, 1.0)
    } else {
        Color::srgb(0.35, 0.35, 0.45)
    };
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
