use bevy::prelude::*;

use shared::{
    city::{CityLayout, MapPlot, MapRoad, RoadSide},
    terrain::WorldTerrain,
};

use crate::{
    session::{CursorTerrainHit, EditorSession, EditorUiState, ToolMode, UiActionRequests},
    tools::VisualRefreshFlags,
};

use super::{CityEditorState, PlotToolSettings};

pub fn handle_city_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut city_state: ResMut<CityEditorState>,
    mut actions: ResMut<UiActionRequests>,
    mut ui_state: ResMut<EditorUiState>,
) {
    if ui_state.tool != ToolMode::Road {
        return;
    }

    if keys.just_pressed(KeyCode::Backspace) {
        if city_state.draft_road_points.pop().is_some() {
            ui_state.status = format!(
                "Removed draft point ({} remaining)",
                city_state.draft_road_points.len()
            );
        }
    }
    if keys.just_pressed(KeyCode::Enter) {
        actions.finish_road_draft = true;
    }
    if keys.just_pressed(KeyCode::Escape) {
        actions.clear_road_draft = true;
    }
}

pub fn handle_city_ui_actions(
    cursor_hit: Res<CursorTerrainHit>,
    world: Res<WorldTerrain>,
    mut session: ResMut<EditorSession>,
    mut city_state: ResMut<CityEditorState>,
    mut ui_state: ResMut<EditorUiState>,
    mut actions: ResMut<UiActionRequests>,
    mut flags: ResMut<VisualRefreshFlags>,
) {
    if actions.finish_road_draft {
        if city_state.draft_road_points.len() < 2 {
            ui_state.status = "Road draft needs at least 2 points".to_string();
        } else {
            session.push_undo_snapshot(&world);
            let road = MapRoad {
                id: session.allocate_road_id(),
                points: city_state
                    .draft_road_points
                    .iter()
                    .map(|point| [point.x, point.y])
                    .collect(),
                width: ui_state.road.width.max(1.0),
                road_class: ui_state.road.road_class,
                lane_count: ui_state.road.lane_count.max(1),
                sidewalk_left: ui_state.road.sidewalk_left,
                sidewalk_right: ui_state.road.sidewalk_right,
                sidewalk_width: ui_state.road.sidewalk_width.max(0.0),
                parking_left: false,
                parking_right: false,
                district: normalized_optional_text(&ui_state.road.district),
            };
            session.map_edits.roads.push(road);
            session.mark_edits_dirty();
            city_state.draft_road_points.clear();
            flags.city_layout = true;
            ui_state.status = format!("Added road ({} total)", session.map_edits.roads.len());
        }
        actions.finish_road_draft = false;
    }

    if actions.clear_road_draft {
        city_state.draft_road_points.clear();
        ui_state.status = "Cleared road draft".to_string();
        actions.clear_road_draft = false;
    }

    if actions.delete_nearest_road {
        let point = cursor_hit.0.map(|hit| Vec2::new(hit.x, hit.z));
        if let Some(point) = point {
            let layout = CityLayout::from_map_edits(&session.map_edits);
            if let Some(nearest) = layout.nearest_road_segment(point, ui_state.road.delete_radius) {
                if let Some(index) = session
                    .map_edits
                    .roads
                    .iter()
                    .position(|road| road.id == nearest.road_id)
                {
                    session.push_undo_snapshot(&world);
                    session.map_edits.roads.remove(index);
                    session.mark_edits_dirty();
                    flags.city_layout = true;
                    ui_state.status = "Deleted nearest road".to_string();
                }
            } else {
                ui_state.status = "No road near cursor".to_string();
            }
        } else {
            ui_state.status = "Move the cursor over terrain to delete a road".to_string();
        }
        actions.delete_nearest_road = false;
    }

    if actions.delete_nearest_plot {
        let point = cursor_hit.0.map(|hit| Vec2::new(hit.x, hit.z));
        if let Some(point) = point {
            if let Some(index) = nearest_plot_index(
                &session.map_edits.plots,
                point,
                ui_state.plot.delete_radius.max(0.5),
            ) {
                session.push_undo_snapshot(&world);
                session.map_edits.plots.remove(index);
                session.mark_edits_dirty();
                flags.city_layout = true;
                ui_state.status = "Deleted nearest plot".to_string();
            } else {
                ui_state.status = "No plot near cursor".to_string();
            }
        } else {
            ui_state.status = "Move the cursor over terrain to delete a plot".to_string();
        }
        actions.delete_nearest_plot = false;
    }
}

pub fn handle_city_tool_input(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_hit: Res<CursorTerrainHit>,
    world: Res<WorldTerrain>,
    mut session: ResMut<EditorSession>,
    mut city_state: ResMut<CityEditorState>,
    mut ui_state: ResMut<EditorUiState>,
    mut actions: ResMut<UiActionRequests>,
    mut flags: ResMut<VisualRefreshFlags>,
) {
    if ui_state.pointer_over_ui {
        return;
    }

    match ui_state.tool {
        ToolMode::Road => {
            if mouse_buttons.just_pressed(MouseButton::Right) {
                if city_state.draft_road_points.len() >= 2 {
                    actions.finish_road_draft = true;
                } else if !city_state.draft_road_points.is_empty() {
                    ui_state.status = "Road draft needs at least 2 points".to_string();
                }
                return;
            }

            if !mouse_buttons.just_pressed(MouseButton::Left) {
                return;
            }

            let Some(hit) = cursor_hit.0 else {
                return;
            };
            let raw_point = Vec2::new(hit.x, hit.z);
            let point = snap_road_point(raw_point, &session, &city_state, &ui_state);
            let too_close = city_state
                .draft_road_points
                .last()
                .map(|last| last.distance(point) < 0.75)
                .unwrap_or(false);
            if too_close {
                ui_state.status = "Road point too close to previous point".to_string();
                return;
            }
            city_state.draft_road_points.push(point);
            let snapped = point.distance(raw_point) > 0.05;
            ui_state.status = if snapped {
                format!(
                    "Road draft: {} points (snapped)",
                    city_state.draft_road_points.len()
                )
            } else {
                format!("Road draft: {} points", city_state.draft_road_points.len())
            };
        }
        ToolMode::Plot => {
            if !mouse_buttons.just_pressed(MouseButton::Left) {
                return;
            }
            let Some(hit) = cursor_hit.0 else {
                return;
            };
            session.push_undo_snapshot(&world);
            let placements =
                planned_plot_placements(Vec2::new(hit.x, hit.z), &ui_state.plot, &session);
            if placements.is_empty() {
                ui_state.status = "No plot placement resolved".to_string();
                return;
            }

            for mut plot in placements {
                plot.id = session.allocate_plot_id();
                session.map_edits.plots.push(plot);
            }
            session.mark_edits_dirty();
            flags.city_layout = true;
            ui_state.status = format!("Added {} plot(s)", ui_state.plot.repeat_count.max(1));
        }
        _ => {}
    }
}

pub fn planned_plot_placements(
    cursor_xz: Vec2,
    settings: &PlotToolSettings,
    session: &EditorSession,
) -> Vec<MapPlot> {
    let layout = CityLayout::from_map_edits(&session.map_edits);
    let tags = parse_tags(&settings.tags_csv);
    let archetypes = vec![settings.primary_archetype];
    let repeat_count = settings.repeat_count.max(1) as usize;

    let mut base_center = cursor_xz;
    let mut tangent = Vec2::new(
        settings.rotation_degrees.to_radians().cos(),
        settings.rotation_degrees.to_radians().sin(),
    );
    let mut rotation_degrees = settings.rotation_degrees;
    let mut frontage_road_id = None;
    let mut driveway_side = None;

    if settings.align_to_nearest_road {
        if let Some(nearest) = layout.nearest_road_segment(cursor_xz, settings.snap_distance) {
            tangent = nearest.tangent;
            rotation_degrees = tangent.y.atan2(tangent.x).to_degrees();
            frontage_road_id = Some(nearest.road_id);
            let side = nearest.side.unwrap_or(RoadSide::Left);
            driveway_side = Some(side);
            let side_sign = match side {
                RoadSide::Left => 1.0,
                RoadSide::Right => -1.0,
            };
            base_center = nearest.closest_point
                + nearest.normal
                    * side_sign
                    * (nearest.width * 0.5
                        + nearest.sidewalk_width
                        + settings.setback
                        + settings.half_extents.y);
        }
    }

    tangent = tangent.normalize_or_zero();
    if tangent.length_squared() <= 1e-6 {
        tangent = Vec2::X;
    }

    let stride = settings.half_extents.x * 2.0 + settings.spacing.max(0.0);
    let offset_origin = -0.5 * stride * (repeat_count.saturating_sub(1) as f32);
    let mut out = Vec::with_capacity(repeat_count);

    for index in 0..repeat_count {
        let center = base_center + tangent * (offset_origin + stride * index as f32);
        out.push(MapPlot {
            id: 0,
            center: [center.x, center.y],
            half_extents: [settings.half_extents.x, settings.half_extents.y],
            rotation_degrees,
            zone: settings.zone,
            frontage_road_id,
            setback: settings.setback.max(0.0),
            driveway_side,
            archetypes: archetypes.clone(),
            building_kind: settings.selected_building,
            tags: tags.clone(),
        });
    }

    out
}

pub(crate) fn snap_road_point(
    point: Vec2,
    session: &EditorSession,
    city_state: &CityEditorState,
    ui_state: &EditorUiState,
) -> Vec2 {
    if !ui_state.road.snap_to_endpoints {
        return point;
    }

    let mut best = None;
    let mut best_dist = ui_state.road.endpoint_snap_distance.max(0.0);

    for road in &session.map_edits.roads {
        for endpoint in &road.points {
            let endpoint = Vec2::new(endpoint[0], endpoint[1]);
            let dist = endpoint.distance(point);
            if dist <= best_dist {
                best = Some(endpoint);
                best_dist = dist;
            }
        }
    }

    for endpoint in &city_state.draft_road_points {
        let dist = endpoint.distance(point);
        if dist <= best_dist {
            best = Some(*endpoint);
            best_dist = dist;
        }
    }

    best.unwrap_or(point)
}

fn nearest_plot_index(plots: &[MapPlot], point: Vec2, max_distance: f32) -> Option<usize> {
    let mut best_index = None;
    let mut best_distance = max_distance;

    for (index, plot) in plots.iter().enumerate() {
        let rect = shared::city::plot_rect(plot);
        let distance = if rect.contains_point(point) {
            0.0
        } else {
            point.distance(rect.center)
        };
        if distance <= best_distance {
            best_distance = distance;
            best_index = Some(index);
        }
    }

    best_index
}

fn parse_tags(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalized_optional_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
