use bevy::prelude::*;

use shared::city::{
    suggested_plot_half_extents, CityBuildingKind, PlotArchetype, PlotZone, RoadClass,
};

#[derive(Debug, Clone)]
pub struct RoadToolSettings {
    pub road_class: RoadClass,
    pub width: f32,
    pub lane_count: u8,
    pub sidewalk_left: bool,
    pub sidewalk_right: bool,
    pub sidewalk_width: f32,
    pub snap_to_endpoints: bool,
    pub endpoint_snap_distance: f32,
    pub delete_radius: f32,
    pub district: String,
}

impl Default for RoadToolSettings {
    fn default() -> Self {
        let road_class = RoadClass::Local;
        Self {
            road_class,
            width: road_class.default_width(),
            lane_count: road_class.default_lane_count(),
            sidewalk_left: true,
            sidewalk_right: true,
            sidewalk_width: road_class.default_sidewalk_width(),
            snap_to_endpoints: true,
            endpoint_snap_distance: 4.0,
            delete_radius: 10.0,
            district: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlotToolSettings {
    pub zone: PlotZone,
    pub primary_archetype: PlotArchetype,
    pub selected_building: Option<CityBuildingKind>,
    pub auto_fit_selected_building: bool,
    pub half_extents: Vec2,
    pub rotation_degrees: f32,
    pub align_to_nearest_road: bool,
    pub snap_distance: f32,
    pub setback: f32,
    pub repeat_count: u32,
    pub spacing: f32,
    pub delete_radius: f32,
    pub tags_csv: String,
}

impl Default for PlotToolSettings {
    fn default() -> Self {
        let mut settings = Self {
            zone: PlotZone::Residential,
            primary_archetype: PlotArchetype::ApartmentLowrise,
            selected_building: Some(CityBuildingKind::LogCabin),
            auto_fit_selected_building: true,
            half_extents: Vec2::new(8.0, 14.0),
            rotation_degrees: 0.0,
            align_to_nearest_road: true,
            snap_distance: 24.0,
            setback: 5.0,
            repeat_count: 1,
            spacing: 2.0,
            delete_radius: 12.0,
            tags_csv: String::new(),
        };
        settings.apply_selected_building_defaults();
        settings
    }
}

impl PlotToolSettings {
    pub fn apply_selected_building_geometry(&mut self) {
        let Some(kind) = self.selected_building else {
            return;
        };
        let spec = kind.spec();
        self.half_extents = suggested_plot_half_extents(kind);
        self.setback = spec.recommended_setback;
        self.spacing = spec.recommended_spacing;
    }

    pub fn apply_selected_building_defaults(&mut self) {
        let Some(kind) = self.selected_building else {
            return;
        };
        let spec = kind.spec();
        self.zone = spec.recommended_zone;
        self.primary_archetype = spec.recommended_archetype;
        self.apply_selected_building_geometry();
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub struct CityEditorState {
    pub draft_road_points: Vec<Vec2>,
}

#[derive(Component)]
pub struct EditorRoadVisual;

#[derive(Component)]
pub struct EditorPlotVisual;

#[derive(Component)]
pub struct EditorPlotBuildingVisual;

#[derive(Component)]
pub struct EditorCityPreviewVisual;
