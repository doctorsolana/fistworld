use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use shared::props::ALL_PROP_KINDS;

use crate::camera::{EditorCameraController, EditorCameraMode};
use crate::city;
use crate::session::{
    CursorTerrainHit, EditorEnvironmentState, EditorMainCamera, EditorSession, EditorUiState,
    ForestBrushPreset, TerrainBrushMode, ToolMode, UiActionRequests,
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
    city_state: Res<city::CityEditorState>,
    catalog: Option<Res<EditorPropCatalog>>,
    mut camera_query: Query<&mut EditorCameraController, With<EditorMainCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    apply_editor_style(ctx);

    egui::TopBottomPanel::top("editor_top_bar")
        .exact_height(52.0)
        .frame(editor_bar_frame())
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(
                    egui::RichText::new("CitySim Editor")
                        .heading()
                        .color(egui::Color32::from_rgb(230, 238, 246)),
                );
                ui.separator();
                draw_chip(ui, "Map", &session.map_id);
                draw_chip(
                    ui,
                    "State",
                    if session.dirty_map || session.dirty_edits {
                        "Unsaved"
                    } else {
                        "Saved"
                    },
                );
                draw_chip(
                    ui,
                    "Counts",
                    &format!(
                        "{} roads / {} plots / {} props",
                        session.map_edits.roads.len(),
                        session.map_edits.plots.len(),
                        session.map_definition.objects.len()
                    ),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Redo").clicked() {
                        actions.redo = true;
                    }
                    if ui.button("Undo").clicked() {
                        actions.undo = true;
                    }
                    if ui.button("Save").clicked() {
                        actions.save = true;
                    }
                });
            });
        });

    egui::SidePanel::left("editor_tool_nav")
        .resizable(false)
        .exact_width(250.0)
        .frame(editor_panel_frame())
        .show(ctx, |ui| {
            draw_tool_palette(ui, &mut ui_state);
        });

    egui::SidePanel::right("editor_inspector")
        .resizable(true)
        .default_width(430.0)
        .min_width(360.0)
        .frame(editor_panel_frame())
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_inspector_header(ui, ui_state.tool);

                    draw_section(ui, "Tool Settings", |ui| {
                        draw_active_tool_settings(
                            ui,
                            &mut ui_state,
                            &mut actions,
                            &city_state,
                            catalog.as_deref(),
                        );
                    });

                    draw_section(ui, "View", |ui| {
                        if let Ok(mut camera) = camera_query.single_mut() {
                            ui.horizontal_wrapped(|ui| {
                                ui.selectable_value(
                                    &mut camera.mode,
                                    EditorCameraMode::Rts,
                                    "Top-Down",
                                );
                                ui.selectable_value(
                                    &mut camera.mode,
                                    EditorCameraMode::Free,
                                    "Free Camera",
                                );
                            });
                        }
                    });

                    draw_section(ui, "Environment", |ui| {
                        draw_environment_controls(
                            ui,
                            &mut env_state,
                            &mut session,
                            &mut refresh_flags,
                        );
                    });

                    draw_section(ui, "Session", |ui| {
                        ui.label(format!("Undo steps: {}", session.undo.len()));
                        ui.label(format!("Redo steps: {}", session.redo.len()));
                        ui.label(format!(
                            "Terrain delta chunks: {}",
                            session.map_edits.terrain_deltas.len()
                        ));
                        ui.separator();
                        if ui.button("Reset Map To Blank...").clicked() {
                            ui_state.show_reset_map_confirm = true;
                        }
                    });
                });
        });

    egui::TopBottomPanel::bottom("editor_status_bar")
        .exact_height(34.0)
        .frame(editor_bar_frame())
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let cursor = cursor_hit
                    .0
                    .map(|hit| format!("x={:.1} y={:.1} z={:.1}", hit.x, hit.y, hit.z))
                    .unwrap_or_else(|| "off terrain".to_string());
                draw_chip(ui, "Cursor", &cursor);
                draw_chip(ui, "Tool", active_tool_heading(ui_state.tool));
                draw_chip(ui, "Status", &ui_state.status);
                draw_chip(
                    ui,
                    "World",
                    &format!(
                        "water {} / {:.1}h",
                        if env_state.show_water {
                            format!("{:.1}m", env_state.water_level)
                        } else {
                            "off".to_string()
                        },
                        env_state.day_time_hours
                    ),
                );
            });
        });

    if ui_state.show_reset_map_confirm {
        let mut keep_open = true;
        egui::Window::new("Confirm Map Reset")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Reset the current map to a blank flat 0.0 state?");
                ui.small(
                    "This clears terrain edits, water, props, roads, plots, markers, player spawn, NPC groups, and blockers. You can still undo it immediately.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                    if ui.button("Yes, Reset Map").clicked() {
                        actions.reset_map_to_blank = true;
                        keep_open = false;
                    }
                });
            });
        ui_state.show_reset_map_confirm = keep_open;
    }

    ui_state.pointer_over_ui = ctx.wants_pointer_input() || ctx.is_pointer_over_area();
}

fn apply_editor_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = egui::Color32::from_rgb(11, 15, 20);
    style.visuals.panel_fill = egui::Color32::from_rgb(9, 13, 18);
    style.visuals.faint_bg_color = egui::Color32::from_rgb(22, 30, 40);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(6, 9, 13);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(24, 34, 45);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(39, 55, 70);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(48, 74, 90);
    style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(214, 224, 232);
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(72, 118, 136);
    style.visuals.selection.stroke.color = egui::Color32::from_rgb(236, 244, 242);
    ctx.set_style(style);
}

fn editor_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(8, 12, 17))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(34, 48, 60)))
        .inner_margin(egui::Margin::symmetric(14, 8))
}

fn editor_panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(11, 16, 22))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(32, 45, 56)))
        .inner_margin(egui::Margin::symmetric(12, 12))
}

fn draw_chip(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(19, 28, 38))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(46, 62, 74)))
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .color(egui::Color32::from_rgb(141, 162, 176)),
                );
                ui.label(egui::RichText::new(value).color(egui::Color32::from_rgb(230, 236, 240)));
            });
        });
}

fn draw_tool_palette(ui: &mut egui::Ui, ui_state: &mut EditorUiState) {
    ui.label(
        egui::RichText::new("Tools")
            .heading()
            .color(egui::Color32::from_rgb(232, 239, 244)),
    );
    ui.add_space(8.0);
    draw_tool_group(ui, "Terrain & Layout", |ui| {
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::Terrain, "Terrain");
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::Road, "Road Network");
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::Plot, "Plots & Buildings");
    });
    draw_tool_group(ui, "Props", |ui| {
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::PlaceProp, "Place Prop");
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::ForestBrush,
            "Forest Brush",
        );
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::EraseProp, "Erase Prop");
    });
    draw_tool_group(ui, "Gameplay", |ui| {
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::SetPlayerSpawn,
            "Player Spawn",
        );
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::PlaceSpawnMarker,
            "Spawn Marker",
        );
    });
}

fn draw_tool_group(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .small()
            .strong()
            .color(egui::Color32::from_rgb(149, 170, 184)),
    );
    ui.add_space(2.0);
    add_contents(ui);
}

fn draw_inspector_header(ui: &mut egui::Ui, tool: ToolMode) {
    ui.label(
        egui::RichText::new(active_tool_heading(tool))
            .heading()
            .color(egui::Color32::from_rgb(232, 239, 244)),
    );
    ui.label(
        egui::RichText::new(active_tool_subtitle(tool))
            .small()
            .color(egui::Color32::from_rgb(151, 169, 181)),
    );
    ui.add_space(10.0);
}

fn draw_section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(15, 22, 30))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(36, 50, 62)))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .outer_margin(egui::Margin::symmetric(0, 6))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .color(egui::Color32::from_rgb(219, 228, 235)),
            );
            ui.add_space(6.0);
            add_contents(ui);
        });
}

fn draw_environment_controls(
    ui: &mut egui::Ui,
    env_state: &mut EditorEnvironmentState,
    session: &mut EditorSession,
    refresh_flags: &mut VisualRefreshFlags,
) {
    let mut water_level_changed = false;
    if ui
        .checkbox(&mut env_state.show_water, "Show Water")
        .changed()
    {
        refresh_flags.water_all = true;
        water_level_changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut env_state.water_level, -20.0..=128.0).text("Water Level"))
        .changed()
    {
        water_level_changed = true;
        refresh_flags.water_all = true;
        env_state.show_water = true;
    }
    if water_level_changed {
        session.map_definition.terrain.water_level =
            env_state.show_water.then_some(env_state.water_level);
        session.mark_map_dirty();
    }

    ui.add(egui::Slider::new(&mut env_state.day_time_hours, 0.0..=24.0).text("Time of Day"));
    ui.horizontal_wrapped(|ui| {
        if ui.button("Morning").clicked() {
            env_state.day_time_hours = 9.0;
        }
        if ui.button("Noon").clicked() {
            env_state.day_time_hours = 12.0;
        }
        if ui.button("Afternoon").clicked() {
            env_state.day_time_hours = 15.0;
        }
        if ui.button("Sunset").clicked() {
            env_state.day_time_hours = 18.0;
        }
    });
}

fn draw_active_tool_settings(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUiState,
    actions: &mut UiActionRequests,
    city_state: &city::CityEditorState,
    catalog: Option<&EditorPropCatalog>,
) {
    match ui_state.tool {
        ToolMode::Terrain => {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut ui_state.terrain_mode, TerrainBrushMode::Raise, "Raise");
                ui.selectable_value(&mut ui_state.terrain_mode, TerrainBrushMode::Lower, "Lower");
                ui.selectable_value(
                    &mut ui_state.terrain_mode,
                    TerrainBrushMode::Flatten,
                    "Flatten",
                );
            });
            ui.add(egui::Slider::new(&mut ui_state.brush_radius, 1.0..=64.0).text("Brush Size"));
            draw_brush_size_presets(ui, &mut ui_state.brush_radius);
            ui.add(
                egui::Slider::new(&mut ui_state.brush_strength, 0.1..=30.0).text("Height Strength"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.flatten_blend, 0.1..=12.0).text("Flatten Blend"),
            );
        }
        ToolMode::PlaceProp => {
            ui.label(format!("Selected: {}", ui_state.selected_asset_label()));
            ui.add(
                egui::TextEdit::singleline(&mut ui_state.prop_search)
                    .hint_text("Search assets")
                    .desired_width(f32::INFINITY),
            );
            draw_prop_catalog(ui, ui_state, catalog);
            ui.separator();
            ui.add(egui::Slider::new(&mut ui_state.prop_scale, 0.1..=8.0).text("Scale"));
            ui.add(egui::Slider::new(&mut ui_state.prop_rotation_degrees, 0.0..=360.0).text("Yaw"));
        }
        ToolMode::ForestBrush => {
            ui.label(egui::RichText::new("Brush").strong());
            ui.add(egui::Slider::new(&mut ui_state.forest.radius, 4.0..=96.0).text("Radius"));
            draw_forest_size_presets(ui, &mut ui_state.forest.radius);
            ui.add(
                egui::Slider::new(&mut ui_state.forest.density_per_100m2, 0.2..=7.5)
                    .text("Density / 100m2"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.forest.min_spacing, 0.75..=8.0).text("Min Spacing"),
            );

            ui.separator();
            ui.label(egui::RichText::new("Preset").strong());
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut ui_state.forest.preset,
                    ForestBrushPreset::Mixed,
                    "Mixed",
                );
                ui.selectable_value(
                    &mut ui_state.forest.preset,
                    ForestBrushPreset::Broadleaf,
                    "Broadleaf",
                );
                ui.selectable_value(&mut ui_state.forest.preset, ForestBrushPreset::Pine, "Pine");
                ui.selectable_value(
                    &mut ui_state.forest.preset,
                    ForestBrushPreset::Deadwood,
                    "Deadwood",
                );
            });

            ui.separator();
            ui.label(egui::RichText::new("Makeup").strong());
            ui.add(egui::Slider::new(&mut ui_state.forest.tree_weight, 0.0..=1.0).text("Trees"));
            ui.add(egui::Slider::new(&mut ui_state.forest.bush_weight, 0.0..=1.0).text("Bushes"));
            ui.add(egui::Slider::new(&mut ui_state.forest.rock_weight, 0.0..=1.0).text("Rocks"));
            ui.add(
                egui::Slider::new(&mut ui_state.forest.ground_cover_weight, 0.0..=1.0)
                    .text("Flowers / Leaves"),
            );

            ui.separator();
            ui.label(egui::RichText::new("Placement").strong());
            ui.add(egui::Slider::new(&mut ui_state.forest.base_scale, 0.35..=2.6).text("Scale"));
            ui.add(
                egui::Slider::new(&mut ui_state.forest.scale_jitter, 0.0..=0.65)
                    .text("Scale Jitter"),
            );
            ui.add(egui::Slider::new(&mut ui_state.forest.max_slope, 0.2..=4.0).text("Max Slope"));
            ui.checkbox(&mut ui_state.forest.avoid_water, "Avoid Water");

            let area = std::f32::consts::PI * ui_state.forest.radius * ui_state.forest.radius;
            let estimate = ((area / 100.0) * ui_state.forest.density_per_100m2).round() as usize;
            ui.small(format!("Estimated stamp: {} props", estimate.min(300)));
        }
        ToolMode::EraseProp => {
            ui.add(egui::Slider::new(&mut ui_state.brush_radius, 1.0..=32.0).text("Erase Radius"));
            draw_brush_size_presets(ui, &mut ui_state.brush_radius);
        }
        ToolMode::Road | ToolMode::Plot => {
            city::draw_city_tool_controls(ui, ui_state, actions, city_state);
        }
        ToolMode::SetPlayerSpawn => {
            ui.label("Click terrain to set the map player spawn.");
        }
        ToolMode::PlaceSpawnMarker => {
            ui.horizontal_wrapped(|ui| {
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
}

fn draw_prop_catalog(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUiState,
    catalog: Option<&EditorPropCatalog>,
) {
    let Some(catalog) = catalog else {
        ui.small("Asset catalog not ready.");
        return;
    };

    ui.small(format!(
        "{} GLBs / {} mapped / {} direct",
        catalog.discovered_asset_count, catalog.mapped_count, catalog.unmapped_count
    ));

    let search = ui_state.prop_search.trim().to_ascii_lowercase();
    let mut any_match = false;

    egui::ScrollArea::vertical()
        .id_salt("editor_prop_catalog")
        .max_height(300.0)
        .show(ui, |ui| {
            for category in &catalog.categories {
                let matching: Vec<&PropCatalogAssetEntry> = category
                    .assets
                    .iter()
                    .filter(|asset| asset_matches_search(asset, search.as_str()))
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                any_match = true;

                egui::CollapsingHeader::new(format!("{} ({})", category.name, matching.len()))
                    .show(ui, |ui| {
                        for asset in matching {
                            let selected = if let Some(mapped_index) = asset.mapped_index {
                                ui_state.selected_custom_scene.is_none()
                                    && ui_state.selected_prop_index == mapped_index
                            } else {
                                ui_state.selected_custom_scene.as_deref()
                                    == Some(asset.scene_path.as_str())
                            };

                            let label = if let Some(mapped_index) = asset.mapped_index {
                                format!(
                                    "{} ({})",
                                    asset.display_name,
                                    ALL_PROP_KINDS[mapped_index].id()
                                )
                            } else {
                                format!("{} [direct]", asset.display_name)
                            };

                            if ui.selectable_label(selected, label).clicked() {
                                if let Some(mapped_index) = asset.mapped_index {
                                    ui_state.selected_prop_index = mapped_index;
                                    ui_state.selected_custom_scene = None;
                                } else {
                                    ui_state.selected_custom_scene = Some(asset.scene_path.clone());
                                }
                            }
                        }
                    });
            }
        });

    if !any_match {
        ui.small("No assets match this filter.");
    }
}

fn draw_tool_button(ui: &mut egui::Ui, current_tool: &mut ToolMode, tool: ToolMode, label: &str) {
    let selected = *current_tool == tool;
    let button = egui::Button::new(label).selected(selected);
    if ui.add_sized([ui.available_width(), 32.0], button).clicked() {
        *current_tool = tool;
    }
}

fn draw_brush_size_presets(ui: &mut egui::Ui, brush_radius: &mut f32) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Quick Size");
        for preset in [2.0, 4.0, 8.0, 16.0, 32.0] {
            if ui.button(format!("{preset:.0}m")).clicked() {
                *brush_radius = preset;
            }
        }
    });
}

fn draw_forest_size_presets(ui: &mut egui::Ui, brush_radius: &mut f32) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Quick Size");
        for preset in [8.0, 16.0, 24.0, 40.0, 64.0] {
            if ui.button(format!("{preset:.0}m")).clicked() {
                *brush_radius = preset;
            }
        }
    });
}

fn active_tool_heading(tool: ToolMode) -> &'static str {
    match tool {
        ToolMode::Terrain => "Terrain",
        ToolMode::PlaceProp => "Prop Placement",
        ToolMode::ForestBrush => "Forest Brush",
        ToolMode::EraseProp => "Prop Erase",
        ToolMode::Road => "Road",
        ToolMode::Plot => "Plot",
        ToolMode::SetPlayerSpawn => "Player Spawn",
        ToolMode::PlaceSpawnMarker => "Spawn Marker",
    }
}

fn active_tool_subtitle(tool: ToolMode) -> &'static str {
    match tool {
        ToolMode::Terrain => "Height sculpting and flattening.",
        ToolMode::PlaceProp => "Scene asset placement.",
        ToolMode::ForestBrush => "Biased foliage, rocks, and ground cover.",
        ToolMode::EraseProp => "Prop cleanup by radius.",
        ToolMode::Road => "Road drafting and street defaults.",
        ToolMode::Plot => "Lots, zones, and building placement.",
        ToolMode::SetPlayerSpawn => "Map start position.",
        ToolMode::PlaceSpawnMarker => "NPC, POI, and player markers.",
    }
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
