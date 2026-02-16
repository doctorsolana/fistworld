use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use shared::props::ALL_PROP_KINDS;

use crate::camera::{EditorCameraController, EditorCameraMode};
use crate::session::{
    CursorTerrainHit, EditorEnvironmentState, EditorMainCamera, EditorSession, EditorUiState,
    TerrainBrushMode, ToolMode, UiActionRequests,
};
use crate::tools::VisualRefreshFlags;

#[derive(Debug, Clone)]
pub struct PropCatalogCategory {
    pub name: String,
    pub assets: Vec<PropCatalogAssetEntry>,
}

#[derive(Debug, Clone)]
pub struct PropCatalogAssetEntry {
    pub scene_path: String,
    pub display_name: String,
    pub mapped_index: Option<usize>,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct EditorPropCatalog {
    pub categories: Vec<PropCatalogCategory>,
    pub discovered_asset_count: usize,
    pub mapped_count: usize,
    pub unmapped_count: usize,
}

pub fn build_prop_catalog(mut commands: Commands) {
    let asset_root = editor_asset_root();
    let discovered_assets = discover_glb_assets(&asset_root);
    let known_scene_paths: HashMap<String, usize> = ALL_PROP_KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            (
                scene_path_without_scene(kind.scene_path()).to_string(),
                index,
            )
        })
        .collect();

    let mut grouped: BTreeMap<String, Vec<PropCatalogAssetEntry>> = BTreeMap::new();
    let mut seen = HashSet::new();
    for path in &discovered_assets {
        let category = asset_category_from_path(path);
        let entry = PropCatalogAssetEntry {
            scene_path: path.clone(),
            display_name: asset_display_name(path),
            mapped_index: known_scene_paths.get(path).copied(),
        };
        grouped.entry(category).or_default().push(entry);
        seen.insert(path.clone());
    }

    // Keep mapped items visible even if discovery misses a file.
    for (path, index) in &known_scene_paths {
        if seen.contains(path) {
            continue;
        }
        grouped
            .entry(asset_category_from_path(path))
            .or_default()
            .push(PropCatalogAssetEntry {
                scene_path: path.clone(),
                display_name: asset_display_name(path),
                mapped_index: Some(*index),
            });
    }

    let mut mapped_count = 0usize;
    let mut unmapped_count = 0usize;
    let mut categories = Vec::with_capacity(grouped.len());
    for (name, mut assets) in grouped {
        assets.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        for asset in &assets {
            if asset.mapped_index.is_some() {
                mapped_count += 1;
            } else {
                unmapped_count += 1;
            }
        }
        categories.push(PropCatalogCategory { name, assets });
    }

    commands.insert_resource(EditorPropCatalog {
        categories,
        discovered_asset_count: discovered_assets.len(),
        mapped_count,
        unmapped_count,
    });
}

pub fn editor_ui_panel(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<EditorUiState>,
    mut actions: ResMut<UiActionRequests>,
    mut session: ResMut<EditorSession>,
    mut env_state: ResMut<EditorEnvironmentState>,
    mut refresh_flags: ResMut<VisualRefreshFlags>,
    cursor_hit: Res<CursorTerrainHit>,
    catalog: Option<Res<EditorPropCatalog>>,
    mut camera_query: Query<&mut EditorCameraController, With<EditorMainCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::TopBottomPanel::top("editor_top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("Map: {}", session.map_id));
            let dirty = if session.dirty_map || session.dirty_edits {
                "Unsaved"
            } else {
                "Saved"
            };
            ui.separator();
            ui.label(dirty);
            ui.separator();
            if ui.button("Save (Ctrl+S)").clicked() {
                actions.save = true;
            }
            if ui.button("Undo (Ctrl+Z)").clicked() {
                actions.undo = true;
            }
            if ui.button("Redo (Ctrl+Y)").clicked() {
                actions.redo = true;
            }
        });
    });

    egui::Window::new("Tools")
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.heading("Tool Mode");
            if let Ok(mut camera) = camera_query.single_mut() {
                ui.separator();
                ui.heading("Camera");
                ui.radio_value(&mut camera.mode, EditorCameraMode::Rts, "Top-Down RTS");
                ui.radio_value(&mut camera.mode, EditorCameraMode::Free, "Free Camera");
                ui.small("WASD pan in RTS mode, wheel zoom, F5 toggles mode.");
            }

            ui.separator();
            ui.heading("Environment");
            let mut water_level_changed = false;
            if ui.checkbox(&mut env_state.show_water, "Show Water").changed() {
                refresh_flags.water_all = true;
                water_level_changed = true;
            }
            if ui
                .add(
                    egui::Slider::new(&mut env_state.water_level, -20.0..=128.0)
                        .text("Water Level"),
                )
                .changed()
            {
                water_level_changed = true;
                refresh_flags.water_all = true;
                env_state.show_water = true;
            }
            if water_level_changed {
                session.map_definition.terrain.water_level = Some(env_state.water_level);
                session.mark_map_dirty();
            }
            if ui
                .add(egui::Slider::new(&mut env_state.day_time_hours, 0.0..=24.0).text("Time of Day"))
                .changed()
            {
                ui_state.status = format!("Time set to {:.2}", env_state.day_time_hours);
            }

            ui.separator();
            ui.radio_value(&mut ui_state.tool, ToolMode::Terrain, "Terrain");
            ui.radio_value(&mut ui_state.tool, ToolMode::PlaceProp, "Place Prop");
            ui.radio_value(&mut ui_state.tool, ToolMode::EraseProp, "Erase Prop");
            ui.radio_value(
                &mut ui_state.tool,
                ToolMode::SetPlayerSpawn,
                "Set Player Spawn",
            );
            ui.radio_value(
                &mut ui_state.tool,
                ToolMode::PlaceSpawnMarker,
                "Place Spawn Marker",
            );

            ui.separator();
            match ui_state.tool {
                ToolMode::Terrain => {
                    ui.label("Terrain Brush");
                    ui.radio_value(&mut ui_state.terrain_mode, TerrainBrushMode::Raise, "Raise");
                    ui.radio_value(&mut ui_state.terrain_mode, TerrainBrushMode::Lower, "Lower");
                    ui.radio_value(
                        &mut ui_state.terrain_mode,
                        TerrainBrushMode::Flatten,
                        "Flatten",
                    );
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_radius, 1.0..=64.0)
                            .text("Radius (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_strength, 0.1..=30.0)
                            .text("Strength"),
                    );
                    ui.add(
                        egui::Slider::new(&mut ui_state.flatten_blend, 0.1..=12.0)
                            .text("Flatten Blend"),
                    );
                }
                ToolMode::PlaceProp => {
                    ui.label("Prop Placement");
                    ui.label(format!("Selected: {}", ui_state.selected_asset_label()));
                    if ui_state.selected_custom_scene.is_some() {
                        ui.small(
                            "Unmapped asset selected: placement is enabled using direct scene path. It will render in-game but may not have baked collider/typed tuning until mapped in shared props.",
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label("Filter");
                        ui.text_edit_singleline(&mut ui_state.prop_search);
                    });

                    if let Some(catalog) = catalog.as_ref() {
                        ui.small(format!(
                            "Client GLBs: {} | Mapped: {} | Unmapped: {}",
                            catalog.discovered_asset_count,
                            catalog.mapped_count,
                            catalog.unmapped_count
                        ));

                        let search = ui_state.prop_search.trim().to_ascii_lowercase();
                        let mut any_match = false;

                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .show(ui, |ui| {
                                for category in &catalog.categories {
                                    let matching: Vec<&PropCatalogAssetEntry> = category
                                        .assets
                                        .iter()
                                        .filter(|asset| {
                                            asset_matches_search(asset, search.as_str())
                                        })
                                        .collect();
                                    if matching.is_empty() {
                                        continue;
                                    }
                                    any_match = true;

                                    egui::CollapsingHeader::new(format!(
                                        "{} ({})",
                                        category.name,
                                        matching.len()
                                    ))
                                    .show(ui, |ui| {
                                        for asset in matching {
                                            let selected = if let Some(mapped_index) =
                                                asset.mapped_index
                                            {
                                                ui_state.selected_custom_scene.is_none()
                                                    && ui_state.selected_prop_index == mapped_index
                                            } else {
                                                ui_state.selected_custom_scene.as_deref()
                                                    == Some(asset.scene_path.as_str())
                                            };

                                            let label = if let Some(mapped_index) =
                                                asset.mapped_index
                                            {
                                                format!(
                                                    "{} ({})",
                                                    asset.display_name,
                                                    ALL_PROP_KINDS[mapped_index].id()
                                                )
                                            } else {
                                                format!("{} [direct-path]", asset.display_name)
                                            };

                                            if ui.selectable_label(selected, label).clicked() {
                                                if let Some(mapped_index) = asset.mapped_index {
                                                    ui_state.selected_prop_index = mapped_index;
                                                    ui_state.selected_custom_scene = None;
                                                } else {
                                                    ui_state.selected_custom_scene =
                                                        Some(asset.scene_path.clone());
                                                }
                                            }
                                        }
                                    });
                                }
                            });

                        if !any_match {
                            ui.small("No assets match this filter.");
                        }
                    } else {
                        ui.small("Asset catalog not ready.");
                    }

                    ui.add(egui::Slider::new(&mut ui_state.prop_scale, 0.1..=8.0).text("Scale"));
                    ui.add(
                        egui::Slider::new(&mut ui_state.prop_rotation_degrees, 0.0..=360.0)
                            .text("Yaw (deg)"),
                    );
                }
                ToolMode::EraseProp => {
                    ui.label("Erase nearest prop within brush radius.");
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_radius, 1.0..=32.0)
                            .text("Erase Radius (m)"),
                    );
                }
                ToolMode::SetPlayerSpawn => {
                    ui.label("Click terrain to set map player spawn.");
                }
                ToolMode::PlaceSpawnMarker => {
                    ui.label("Spawn Marker Placement");
                    ui.horizontal(|ui| {
                        ui.label("Kind:");
                        ui.selectable_value(
                            &mut ui_state.selected_spawn_kind,
                            shared::map::SpawnMarkerKind::NpcGroup,
                            "NPC Group",
                        );
                        ui.selectable_value(
                            &mut ui_state.selected_spawn_kind,
                            shared::map::SpawnMarkerKind::Poi,
                            "POI",
                        );
                        ui.selectable_value(
                            &mut ui_state.selected_spawn_kind,
                            shared::map::SpawnMarkerKind::Player,
                            "Player",
                        );
                    });
                    ui.add(
                        egui::Slider::new(&mut ui_state.spawn_marker_radius, 1.0..=64.0)
                            .text("Marker Radius"),
                    );
                }
            }

            ui.separator();
            if let Some(hit) = cursor_hit.0 {
                ui.label(format!(
                    "Cursor: x={:.2}, y={:.2}, z={:.2}",
                    hit.x, hit.y, hit.z
                ));
            } else {
                ui.label("Cursor: (off terrain)");
            }
            ui.label(format!(
                "Status: {} | Water: {:.2} | Time: {:.2}h",
                ui_state.status, env_state.water_level, env_state.day_time_hours
            ));
            ui.label("RTS: WASD pan + wheel zoom | Free: RMB look + WASD/QE move");
        });

    ui_state.pointer_over_ui = ctx.wants_pointer_input() || ctx.is_pointer_over_area();
}

fn editor_asset_root() -> PathBuf {
    if let Ok(path) = std::env::var("FISTFORCE_ASSET_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("client/assets")
}

fn discover_glb_assets(asset_root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_glb_recursive(asset_root, &mut files);

    let mut out = Vec::with_capacity(files.len());
    for path in files {
        if let Ok(relative) = path.strip_prefix(asset_root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    out.sort();
    out
}

fn collect_glb_recursive(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_glb_recursive(&path, out);
            continue;
        }
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("glb") {
            out.push(path);
        }
    }
}

fn scene_path_without_scene(path: &str) -> &str {
    path.split('#').next().unwrap_or(path)
}

fn asset_display_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".glb").unwrap_or(file).to_string()
}

fn asset_category_from_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 2 {
        return parts[1..parts.len() - 1].join("/");
    }
    "other".to_string()
}

fn asset_matches_search(asset: &PropCatalogAssetEntry, search: &str) -> bool {
    if search.is_empty() {
        return true;
    }
    asset.display_name.to_ascii_lowercase().contains(search)
        || asset.scene_path.to_ascii_lowercase().contains(search)
}
