use bevy_egui::egui;

use shared::city::{CityBuildingKind, PlotArchetype, PlotZone, RoadClass};

use crate::session::{EditorUiState, ToolMode, UiActionRequests};

use super::CityEditorState;

pub fn draw_city_tool_controls(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUiState,
    actions: &mut UiActionRequests,
    city_state: &CityEditorState,
) {
    match ui_state.tool {
        ToolMode::Road => {
            ui.label("Road Authoring");
            ui.small(
                "LMB adds road points. RMB or Enter finishes. Backspace removes the last point. Nearby road points snap automatically.",
            );

            let previous_class = ui_state.road.road_class;
            ui.horizontal(|ui| {
                ui.label("Class");
                ui.selectable_value(&mut ui_state.road.road_class, RoadClass::Alley, "Alley");
                ui.selectable_value(&mut ui_state.road.road_class, RoadClass::Local, "Local");
                ui.selectable_value(
                    &mut ui_state.road.road_class,
                    RoadClass::Collector,
                    "Collector",
                );
                ui.selectable_value(
                    &mut ui_state.road.road_class,
                    RoadClass::Arterial,
                    "Arterial",
                );
            });
            if ui_state.road.road_class != previous_class {
                apply_road_class_defaults(ui_state);
                ui_state.status = format!(
                    "Applied {} road defaults",
                    road_class_label(ui_state.road.road_class)
                );
            }
            if ui.button("Reset To Class Defaults").clicked() {
                apply_road_class_defaults(ui_state);
                ui_state.status = format!(
                    "Reset to {} road defaults",
                    road_class_label(ui_state.road.road_class)
                );
            }
            ui.add(egui::Slider::new(&mut ui_state.road.width, 3.0..=32.0).text("Road Width"));
            ui.add(egui::Slider::new(&mut ui_state.road.lane_count, 1..=6).text("Lane Count"));
            ui.checkbox(&mut ui_state.road.sidewalk_left, "Left Sidewalk");
            ui.checkbox(&mut ui_state.road.sidewalk_right, "Right Sidewalk");
            ui.add(
                egui::Slider::new(&mut ui_state.road.sidewalk_width, 0.0..=8.0)
                    .text("Sidewalk Width"),
            );
            ui.small("Class changes now refresh width, lanes, and sidewalk width automatically.");
            ui.checkbox(&mut ui_state.road.snap_to_endpoints, "Snap To Road Points");
            ui.add(
                egui::Slider::new(&mut ui_state.road.endpoint_snap_distance, 1.0..=12.0)
                    .text("Point Snap Radius"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.road.delete_radius, 2.0..=32.0)
                    .text("Delete Radius"),
            );
            ui.horizontal(|ui| {
                ui.label("District");
                ui.text_edit_singleline(&mut ui_state.road.district);
            });
            ui.label(format!(
                "Draft points: {}",
                city_state.draft_road_points.len()
            ));
            ui.horizontal(|ui| {
                if ui.button("Finish Draft").clicked() {
                    actions.finish_road_draft = true;
                }
                if ui.button("Clear Draft").clicked() {
                    actions.clear_road_draft = true;
                }
                if ui.button("Delete Nearest").clicked() {
                    actions.delete_nearest_road = true;
                }
            });
        }
        ToolMode::Plot => {
            ui.label("Plot Authoring");
            ui.small(
                "LMB stamps one or more plots. When aligned to a road, the click chooses frontage.",
            );

            let previous_building = ui_state.plot.selected_building;
            egui::ComboBox::from_label("Building")
                .selected_text(
                    ui_state
                        .plot
                        .selected_building
                        .map(CityBuildingKind::display_name)
                        .unwrap_or("Generic Plot"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut ui_state.plot.selected_building, None, "Generic Plot");
                    for kind in CityBuildingKind::all() {
                        ui.selectable_value(
                            &mut ui_state.plot.selected_building,
                            Some(*kind),
                            kind.display_name(),
                        );
                    }
                });
            ui.checkbox(
                &mut ui_state.plot.auto_fit_selected_building,
                "Auto Fit Selected Building",
            );
            if ui_state.plot.selected_building != previous_building
                && ui_state.plot.auto_fit_selected_building
            {
                ui_state.plot.apply_selected_building_geometry();
                ui_state.status = match ui_state.plot.selected_building {
                    Some(kind) => format!("Fit plot to {}", kind.display_name()),
                    None => "Using generic plot sizing".to_string(),
                };
            }
            if ui.button("Apply Building Defaults").clicked() {
                ui_state.plot.apply_selected_building_defaults();
                ui_state.status = match ui_state.plot.selected_building {
                    Some(kind) => format!("Applied {} defaults", kind.display_name()),
                    None => "Generic plot selected".to_string(),
                };
            }
            if let Some(kind) = ui_state.plot.selected_building {
                let spec = kind.spec();
                ui.small(format!(
                    "{} footprint {:.1}m x {:.1}m, suggested setback {:.1}m",
                    spec.display_name, spec.footprint.x, spec.footprint.y, spec.recommended_setback,
                ));
            }

            ui.horizontal(|ui| {
                ui.label("Zone");
                ui.selectable_value(
                    &mut ui_state.plot.zone,
                    PlotZone::Residential,
                    "Residential",
                );
                ui.selectable_value(&mut ui_state.plot.zone, PlotZone::MixedUse, "Mixed");
                ui.selectable_value(&mut ui_state.plot.zone, PlotZone::Commercial, "Commercial");
                ui.selectable_value(&mut ui_state.plot.zone, PlotZone::Industrial, "Industrial");
                ui.selectable_value(&mut ui_state.plot.zone, PlotZone::Civic, "Civic");
                ui.selectable_value(&mut ui_state.plot.zone, PlotZone::Park, "Park");
            });
            ui.horizontal(|ui| {
                ui.label("Archetype");
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::HouseSmall,
                    "House",
                );
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::HouseRow,
                    "Row House",
                );
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::CornerStore,
                    "Store",
                );
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::ApartmentLowrise,
                    "Apartment",
                );
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::Warehouse,
                    "Warehouse",
                );
                ui.selectable_value(
                    &mut ui_state.plot.primary_archetype,
                    PlotArchetype::Civic,
                    "Civic",
                );
            });
            ui.add(
                egui::Slider::new(&mut ui_state.plot.half_extents.x, 2.0..=40.0).text("Half Width"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.plot.half_extents.y, 4.0..=48.0).text("Half Depth"),
            );
            ui.checkbox(
                &mut ui_state.plot.align_to_nearest_road,
                "Align To Nearest Road",
            );
            ui.add(
                egui::Slider::new(&mut ui_state.plot.snap_distance, 4.0..=48.0)
                    .text("Road Snap Distance"),
            );
            ui.add(egui::Slider::new(&mut ui_state.plot.setback, 0.0..=20.0).text("Setback"));
            ui.add(egui::Slider::new(&mut ui_state.plot.repeat_count, 1..=12).text("Plots In Row"));
            ui.add(egui::Slider::new(&mut ui_state.plot.spacing, 0.0..=12.0).text("Gap"));
            ui.add(
                egui::Slider::new(&mut ui_state.plot.rotation_degrees, 0.0..=360.0)
                    .text("Manual Rotation"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.plot.delete_radius, 2.0..=32.0)
                    .text("Delete Radius"),
            );
            ui.horizontal(|ui| {
                ui.label("Tags");
                ui.text_edit_singleline(&mut ui_state.plot.tags_csv);
            });
            if ui.button("Delete Nearest Plot").clicked() {
                actions.delete_nearest_plot = true;
            }
        }
        _ => {}
    }
}

fn apply_road_class_defaults(ui_state: &mut EditorUiState) {
    let road_class = ui_state.road.road_class;
    ui_state.road.width = road_class.default_width();
    ui_state.road.lane_count = road_class.default_lane_count();
    ui_state.road.sidewalk_width = road_class.default_sidewalk_width();
}

fn road_class_label(road_class: RoadClass) -> &'static str {
    match road_class {
        RoadClass::Alley => "alley",
        RoadClass::Local => "local",
        RoadClass::Collector => "collector",
        RoadClass::Arterial => "arterial",
    }
}
