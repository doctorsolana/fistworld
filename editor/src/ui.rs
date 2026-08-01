use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use shared::props::ALL_PROP_KINDS;
use shared::terrain::TerrainLayer;

use crate::camera::{EditorCameraController, EditorCameraMode};
use crate::city;
use crate::session::{
    BrushMix, CursorTerrainHit, EditorEnvironmentState, EditorMainCamera, EditorSession,
    EditorUiState, ForestBrushPreset, RecentAsset, TerrainBrushMode, TerrainEditMode, ToolMode,
    UiActionRequests,
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

    // egui 0.35: Panel::show takes &mut Ui, not &Context, so panels need an
    // explicit background root Ui spanning the viewport.
    let mut root = egui::Ui::new(
        ctx.clone(),
        "editor_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("editor_top_bar")
        .frame(editor_bar_frame())
        .show(&mut root, |ui| {
            // --- Menu bar ---
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Save\t(Ctrl+S)").clicked() {
                        actions.save = true;
                    }
                    if ui.button("Save & Exit").clicked() {
                        actions.save_and_exit = true;
                    }
                    if ui.button("Exit Without Saving").clicked() {
                        actions.exit_without_saving = true;
                    }
                    ui.separator();
                    if ui.button("Reset Map To Blank…").clicked() {
                        ui_state.show_reset_map_confirm = true;
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .button(format!("Undo ({})\t(Ctrl+Z)", session.undo.len()))
                        .clicked()
                    {
                        actions.undo = true;
                    }
                    if ui
                        .button(format!("Redo ({})\t(Ctrl+Y)", session.redo.len()))
                        .clicked()
                    {
                        actions.redo = true;
                    }
                });
                ui.menu_button("View", |ui| {
                    if let Ok(mut camera) = camera_query.single_mut() {
                        ui.selectable_value(&mut camera.mode, EditorCameraMode::Rts, "Top-Down");
                        ui.selectable_value(
                            &mut camera.mode,
                            EditorCameraMode::Free,
                            "Free Camera\t(F5)",
                        );
                    }
                });
                ui.menu_button("World", |ui| {
                    ui.menu_button("Generate World", |ui| {
                        if ui.button("Island…").clicked() {
                            ui_state.pending_generate = Some(crate::worldgen::WorldStyle::Island);
                        }
                        if ui.button("Mainland…").clicked() {
                            ui_state.pending_generate = Some(crate::worldgen::WorldStyle::Mainland);
                        }
                        ui.separator();
                        if ui.button("Great Open World…").clicked() {
                            ui_state.pending_generate = Some(crate::worldgen::WorldStyle::Showcase);
                        }
                        ui.small(
                            "Mountains, bay, islands, lakes, rivers,\nroads and a harbour village.",
                        );
                    });
                    ui.separator();
                    ui.menu_button("Map Size", |ui| {
                        let bounds = session.map_definition.bounds;
                        ui.label(format!(
                            "Current: {:.0} x {:.0} m",
                            bounds.max[0] - bounds.min[0],
                            bounds.max[1] - bounds.min[1]
                        ));
                        ui.separator();
                        for size in [704.0f32, 1408.0, 2112.0, 2816.0] {
                            if ui.button(format!("{size:.0} x {size:.0} m…")).clicked() {
                                ui_state.pending_resize = Some(size * 0.5);
                            }
                        }
                        ui.separator();
                        if ui.button("Custom…").clicked() {
                            ui_state.custom_map_size = session.map_definition.bounds.max[0] * 2.0;
                            ui_state.show_custom_size = true;
                        }
                    });
                });

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

    egui::Panel::left("editor_tool_nav")
        .resizable(false)
        .exact_size(250.0)
        .frame(editor_panel_frame())
        .show(&mut root, |ui| {
            draw_tool_palette(ui, &mut ui_state);
        });

    egui::Panel::right("editor_inspector")
        .resizable(true)
        .default_size(430.0)
        .min_size(360.0)
        .frame(editor_panel_frame())
        .show(&mut root, |ui| {
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

    egui::Panel::bottom("editor_status_bar")
        .exact_size(34.0)
        .frame(editor_bar_frame())
        .show(&mut root, |ui| {
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

    if let Some(style) = ui_state.pending_generate {
        let mut keep_open = true;
        egui::Window::new("Generate Random World")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Generate a random {} world? This replaces the current \
                     map (Ctrl+Z undoes it).",
                    style.label()
                ));
                if style == crate::worldgen::WorldStyle::Showcase {
                    ui.small(
                        "The big one: a mountain range, a great bay with \
                         beaches, an offshore archipelago, inland lakes, \
                         rivers, a harbour village, and roads pathfound \
                         through the valleys. Building the scene takes a \
                         few seconds.",
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Generate").clicked() {
                        let seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 20))
                            .unwrap_or(12345);
                        actions.generate_world = Some((style, seed));
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        if !keep_open {
            ui_state.pending_generate = None;
        }
    }

    if let Some(half) = ui_state.pending_resize {
        let mut keep_open = true;
        egui::Window::new("Resize Map")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Resize the map to {:.0} x {:.0} m? Content outside the \
                     new bounds is removed (Ctrl+Z undoes it).",
                    half * 2.0,
                    half * 2.0
                ));
                ui.horizontal(|ui| {
                    if ui.button("Resize").clicked() {
                        actions.resize_map = Some(half);
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        if !keep_open {
            ui_state.pending_resize = None;
        }
    }

    if ui_state.show_custom_size {
        let mut keep_open = true;
        egui::Window::new("Custom Map Size")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add(
                    egui::Slider::new(&mut ui_state.custom_map_size, 512.0..=2816.0)
                        .step_by(128.0)
                        .text("Size (m)"),
                );
                ui.small(format!(
                    "{:.0} x {:.0} m — {} chunks per side",
                    ui_state.custom_map_size,
                    ui_state.custom_map_size,
                    (ui_state.custom_map_size / 64.0).round() as i32
                ));
                ui.horizontal(|ui| {
                    if ui.button("Apply…").clicked() {
                        ui_state.pending_resize = Some(ui_state.custom_map_size * 0.5);
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        if !keep_open {
            ui_state.show_custom_size = false;
        }
    }

    if ui_state.show_exit_confirm {
        let mut keep_open = true;
        egui::Window::new("Unsaved Changes")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("You have unsaved changes. Save before exiting?");
                ui.horizontal(|ui| {
                    if ui.button("Save & Exit").clicked() {
                        actions.save_and_exit = true;
                        keep_open = false;
                    }
                    if ui.button("Exit Without Saving").clicked() {
                        actions.exit_without_saving = true;
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        ui_state.show_exit_confirm = keep_open;
    }

    ui_state.pointer_over_ui = ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui();
    ui_state.keyboard_captured = ctx.egui_wants_keyboard_input();
}

fn apply_editor_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
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
    ctx.set_global_style(style);
}

fn editor_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(8, 12, 17))
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(34, 48, 60),
        ))
        .inner_margin(egui::Margin::symmetric(14, 8))
}

fn editor_panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(11, 16, 22))
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(32, 45, 56),
        ))
        .inner_margin(egui::Margin::symmetric(12, 12))
}

fn draw_chip(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(19, 28, 38))
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(46, 62, 74),
        ))
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
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::Terrain, "Terrain", "1");
        draw_tool_button(ui, &mut ui_state.tool, ToolMode::Road, "Road Network", "2");
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::Plot,
            "Plots & Buildings",
            "3",
        );
    });
    draw_tool_group(ui, "Props", |ui| {
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::PlaceProp,
            "Place Prop",
            "4",
        );
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::ForestBrush,
            "Scatter Brush",
            "5",
        );
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::EraseProp,
            "Erase Props",
            "6",
        );
    });
    draw_tool_group(ui, "Gameplay", |ui| {
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::SetPlayerSpawn,
            "Player Spawn",
            "7",
        );
        draw_tool_button(
            ui,
            &mut ui_state.tool,
            ToolMode::PlaceSpawnMarker,
            "Spawn Marker",
            "8",
        );
    });
    ui.add_space(10.0);
    ui.small("[ and ] resize the active brush.\nHold LMB to paint with brushes.");
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
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(36, 50, 62),
        ))
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
                ui.selectable_value(
                    &mut ui_state.terrain_edit_mode,
                    TerrainEditMode::Sculpt,
                    "Sculpt",
                );
                ui.selectable_value(
                    &mut ui_state.terrain_edit_mode,
                    TerrainEditMode::Paint,
                    "Paint",
                );
            });
            ui.separator();

            match ui_state.terrain_edit_mode {
                TerrainEditMode::Sculpt => {
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(
                            &mut ui_state.terrain_mode,
                            TerrainBrushMode::Raise,
                            "Raise",
                        );
                        ui.selectable_value(
                            &mut ui_state.terrain_mode,
                            TerrainBrushMode::Lower,
                            "Lower",
                        );
                        ui.selectable_value(
                            &mut ui_state.terrain_mode,
                            TerrainBrushMode::Flatten,
                            "Flatten",
                        );
                    });
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_radius, 1.0..=64.0)
                            .text("Brush Size"),
                    );
                    draw_brush_size_presets(ui, &mut ui_state.brush_radius);
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_strength, 0.1..=30.0)
                            .text("Height Strength"),
                    );
                    ui.add(
                        egui::Slider::new(&mut ui_state.flatten_blend, 0.1..=12.0)
                            .text("Flatten Blend"),
                    );
                }
                TerrainEditMode::Paint => {
                    ui.label("Surface");
                    draw_terrain_layer_swatches(ui, &mut ui_state.terrain_layer);
                    ui.add(
                        egui::Slider::new(&mut ui_state.brush_radius, 1.0..=64.0)
                            .text("Brush Size"),
                    );
                    draw_brush_size_presets(ui, &mut ui_state.brush_radius);
                    ui.add(
                        egui::Slider::new(&mut ui_state.paint_strength, 0.05..=1.0).text("Opacity"),
                    );
                    ui.add(
                        egui::Slider::new(&mut ui_state.paint_softness, 0.0..=0.9)
                            .text("Edge Softness"),
                    );
                }
            }
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
            ui.add(
                egui::Slider::new(&mut ui_state.prop_scale_jitter, 0.0..=0.9).text("Scale Jitter"),
            );
            ui.checkbox(&mut ui_state.prop_random_yaw, "Random Yaw");
            if !ui_state.prop_random_yaw {
                ui.add(
                    egui::Slider::new(&mut ui_state.prop_rotation_degrees, 0.0..=360.0).text("Yaw"),
                );
            }
            ui.checkbox(&mut ui_state.prop_drag_paint, "Drag to Paint");
            if ui_state.prop_drag_paint {
                ui.add(
                    egui::Slider::new(&mut ui_state.prop_drag_spacing, 0.5..=12.0)
                        .text("Paint Spacing"),
                );
                ui.small("Hold LMB and drag to place a prop every few meters.");
            }
        }
        ToolMode::ForestBrush => {
            ui.label(egui::RichText::new("Brush").strong());
            ui.add(egui::Slider::new(&mut ui_state.forest.radius, 4.0..=96.0).text("Radius"));
            draw_forest_size_presets(ui, &mut ui_state.forest.radius);
            ui.add(
                egui::Slider::new(&mut ui_state.forest.density_per_100m2, 0.2..=16.0)
                    .text("Density / 100m2"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.forest.min_spacing, 0.5..=8.0).text("Min Spacing"),
            );
            ui.small("Hold LMB and drag to paint continuously.");

            ui.separator();
            ui.checkbox(
                &mut ui_state.forest.scatter_selected,
                "Paint Selected Asset Only",
            );

            if ui_state.forest.scatter_selected {
                ui.label(format!("Painting: {}", ui_state.selected_asset_label()));
                ui.add(
                    egui::TextEdit::singleline(&mut ui_state.prop_search)
                        .hint_text("Search assets")
                        .desired_width(f32::INFINITY),
                );
                draw_prop_catalog(ui, ui_state, catalog);
            } else {
                ui.label(egui::RichText::new("Species Preset").strong());
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
                    ui.selectable_value(
                        &mut ui_state.forest.preset,
                        ForestBrushPreset::Pine,
                        "Pine",
                    );
                    ui.selectable_value(
                        &mut ui_state.forest.preset,
                        ForestBrushPreset::Deadwood,
                        "Deadwood",
                    );
                });

                ui.separator();
                ui.label(egui::RichText::new("Makeup").strong());
                ui.horizontal_wrapped(|ui| {
                    ui.label("Mix");
                    if ui.button("Forest").clicked() {
                        ui_state.forest.apply_mix(BrushMix::Forest);
                    }
                    if ui.button("Meadow").clicked() {
                        ui_state.forest.apply_mix(BrushMix::Meadow);
                    }
                    if ui.button("Grass Only").clicked() {
                        ui_state.forest.apply_mix(BrushMix::GrassOnly);
                    }
                    if ui.button("Rocky").clicked() {
                        ui_state.forest.apply_mix(BrushMix::Rocky);
                    }
                });
                ui.add(
                    egui::Slider::new(&mut ui_state.forest.tree_weight, 0.0..=1.0).text("Trees"),
                );
                ui.add(
                    egui::Slider::new(&mut ui_state.forest.bush_weight, 0.0..=1.0).text("Bushes"),
                );
                ui.add(
                    egui::Slider::new(&mut ui_state.forest.rock_weight, 0.0..=1.0).text("Rocks"),
                );
                ui.add(
                    egui::Slider::new(&mut ui_state.forest.grass_weight, 0.0..=1.0).text("Grass"),
                );
                ui.add(
                    egui::Slider::new(&mut ui_state.forest.ground_cover_weight, 0.0..=1.0)
                        .text("Flowers / Leaves"),
                );
            }

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
            ui.small("Erases every prop inside the radius. Hold LMB and drag to sweep.");
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

    if !ui_state.recent_assets.is_empty() {
        let recents = ui_state.recent_assets.clone();
        ui.horizontal_wrapped(|ui| {
            ui.small("Recent:");
            for recent in &recents {
                let selected = if let Some(mapped_index) = recent.mapped_index {
                    ui_state.selected_custom_scene.is_none()
                        && ui_state.selected_prop_index == mapped_index
                } else {
                    ui_state.selected_custom_scene.as_deref() == Some(recent.scene_path.as_str())
                };
                if ui
                    .selectable_label(selected, &recent.display_name)
                    .clicked()
                {
                    select_catalog_asset(
                        ui_state,
                        recent.mapped_index,
                        &recent.scene_path,
                        &recent.display_name,
                    );
                }
            }
        });
    }

    let search = ui_state.prop_search.trim().to_ascii_lowercase();
    let force_open = (!search.is_empty()).then_some(true);
    let mut any_match = false;

    egui::ScrollArea::vertical()
        .id_salt("editor_prop_catalog")
        .max_height(420.0)
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
                    .open(force_open)
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
                                select_catalog_asset(
                                    ui_state,
                                    asset.mapped_index,
                                    &asset.scene_path,
                                    &asset.display_name,
                                );
                            }
                        }
                    });
            }
        });

    if !any_match {
        ui.small("No assets match this filter.");
    }
}

fn select_catalog_asset(
    ui_state: &mut EditorUiState,
    mapped_index: Option<usize>,
    scene_path: &str,
    display_name: &str,
) {
    if let Some(mapped_index) = mapped_index {
        ui_state.selected_prop_index = mapped_index;
        ui_state.selected_custom_scene = None;
    } else {
        ui_state.selected_custom_scene = Some(scene_path.to_string());
    }
    ui_state.note_recent_asset(RecentAsset {
        scene_path: scene_path.to_string(),
        display_name: display_name.to_string(),
        mapped_index,
    });
}

fn draw_tool_button(
    ui: &mut egui::Ui,
    current_tool: &mut ToolMode,
    tool: ToolMode,
    label: &str,
    hotkey: &str,
) {
    let selected = *current_tool == tool;
    let button = egui::Button::new(label)
        .shortcut_text(hotkey)
        .selected(selected);
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

fn draw_terrain_layer_swatches(ui: &mut egui::Ui, selected: &mut TerrainLayer) {
    ui.horizontal_wrapped(|ui| {
        // Names and colours come from `shared::terrain::TERRAIN_LAYERS`, which is also what the
        // shader renders. They used to be hand-written here and had drifted badly: Dirt was
        // labelled "Dark Ground" in rgb(70,84,42) and Sand "Dry Dirt" in rgb(157,137,113) --
        // the mean colours of the old photographic textures, not the flat palette the terrain
        // actually draws. Painting "Dark Ground" put brown on the map.
        for def in shared::terrain::TERRAIN_LAYERS.iter() {
            terrain_layer_swatch(ui, selected, def.layer, def.display_name, swatch_color(def));
        }
    });
}

/// Palette colours are linear; egui wants sRGB bytes.
fn swatch_color(def: &shared::terrain::TerrainLayerDef) -> egui::Color32 {
    fn to_srgb_u8(c: f32) -> u8 {
        let s = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s.clamp(0.0, 1.0) * 255.0).round() as u8
    }
    egui::Color32::from_rgb(
        to_srgb_u8(def.color[0]),
        to_srgb_u8(def.color[1]),
        to_srgb_u8(def.color[2]),
    )
}

fn terrain_layer_swatch(
    ui: &mut egui::Ui,
    selected: &mut TerrainLayer,
    layer: TerrainLayer,
    label: &str,
    color: egui::Color32,
) {
    let is_selected = *selected == layer;
    let stroke = if is_selected {
        egui::Stroke::new(2.0_f32, egui::Color32::WHITE)
    } else {
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(110))
    };
    let text_color = if color.r() as u16 + color.g() as u16 + color.b() as u16 > 410 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    };
    let button = egui::Button::new(egui::RichText::new(label).color(text_color))
        .fill(color)
        .stroke(stroke);
    if ui.add_sized([112.0, 34.0], button).clicked() {
        *selected = layer;
    }
}

fn active_tool_heading(tool: ToolMode) -> &'static str {
    match tool {
        ToolMode::Terrain => "Terrain",
        ToolMode::PlaceProp => "Prop Placement",
        ToolMode::ForestBrush => "Scatter Brush",
        ToolMode::EraseProp => "Prop Erase",
        ToolMode::Road => "Road",
        ToolMode::Plot => "Plot",
        ToolMode::SetPlayerSpawn => "Player Spawn",
        ToolMode::PlaceSpawnMarker => "Spawn Marker",
    }
}

fn active_tool_subtitle(tool: ToolMode) -> &'static str {
    match tool {
        ToolMode::Terrain => "Height sculpting and surface painting.",
        ToolMode::PlaceProp => "Single placement with optional drag painting.",
        ToolMode::ForestBrush => "Paint grass, foliage, rocks, or any selected asset.",
        ToolMode::EraseProp => "Sweep-erase every prop inside the brush.",
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
