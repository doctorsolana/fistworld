use std::f32::consts::PI;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::building::{building_rotation_quat, BuildingType};

use super::{MapPlot, OrientedRect, PlotArchetype, PlotZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CityBuildingKind {
    LogCabin,
    Farmstead,
    LumberjackHut,
    #[serde(alias = "TownHall")]
    MootHall,
}

pub const ALL_CITY_BUILDING_KINDS: &[CityBuildingKind] = &[
    CityBuildingKind::LogCabin,
    CityBuildingKind::Farmstead,
    CityBuildingKind::LumberjackHut,
    CityBuildingKind::MootHall,
];

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoredCityBuilding {
    pub plot_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CityBuildingSpec {
    pub kind: CityBuildingKind,
    pub display_name: &'static str,
    pub scene_path: &'static str,
    pub building_type: BuildingType,
    pub recommended_zone: PlotZone,
    pub recommended_archetype: PlotArchetype,
    pub footprint: Vec2,
    pub height: f32,
    pub side_yard: f32,
    pub rear_yard: f32,
    pub recommended_setback: f32,
    pub recommended_spacing: f32,
    pub local_center: Vec2,
    pub base_y: f32,
    pub front_axis: BuildingFrontAxis,
    pub visual_yaw_offset_degrees: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingFrontAxis {
    PositiveZ,
    NegativeZ,
}

impl CityBuildingKind {
    pub fn all() -> &'static [CityBuildingKind] {
        ALL_CITY_BUILDING_KINDS
    }

    pub fn spec(self) -> CityBuildingSpec {
        match self {
            CityBuildingKind::LogCabin => village_spec(
                self,
                "Log Cabin",
                "game_assets/buildings/village/LogCabin.glb#Scene0",
                BuildingType::LogCabin,
                PlotZone::Residential,
                PlotArchetype::HouseSmall,
                Vec2::new(6.0000, 6.9400),
                4.3300,
                1.0,
                2.0,
                1.5,
                1.0,
                Vec2::new(0.0000, 0.0000),
                -0.1600,
                BuildingFrontAxis::PositiveZ,
                0.0,
            ),
            CityBuildingKind::Farmstead => village_spec(
                self,
                "Farmstead",
                "game_assets/buildings/village/Farmstead.glb#Scene0",
                BuildingType::Farmstead,
                PlotZone::Industrial,
                PlotArchetype::Warehouse,
                Vec2::new(5.4100, 6.6200),
                4.0700,
                1.0,
                2.0,
                1.5,
                1.0,
                Vec2::new(0.1400, -0.1700),
                -0.1600,
                BuildingFrontAxis::PositiveZ,
                0.0,
            ),
            CityBuildingKind::LumberjackHut => village_spec(
                self,
                "Lumberjack Hut",
                "game_assets/buildings/village/LumberjackHut.glb#Scene0",
                BuildingType::LumberjackHut,
                PlotZone::Industrial,
                PlotArchetype::Warehouse,
                Vec2::new(5.1600, 5.4000),
                3.7600,
                1.0,
                2.0,
                1.5,
                1.0,
                Vec2::new(0.3300, -0.1800),
                -0.1600,
                BuildingFrontAxis::PositiveZ,
                0.0,
            ),
            CityBuildingKind::MootHall => village_spec(
                self,
                "Moot Hall",
                "game_assets/buildings/village/MootHall.glb#Scene0",
                BuildingType::MootHall,
                PlotZone::Civic,
                PlotArchetype::Civic,
                Vec2::new(6.4500, 8.7400),
                8.1800,
                1.0,
                2.0,
                1.5,
                1.0,
                Vec2::new(0.0000, -0.2800),
                -0.1600,
                BuildingFrontAxis::PositiveZ,
                0.0,
            ),
        }
    }

    pub fn display_name(self) -> &'static str {
        self.spec().display_name
    }
}

fn village_spec(
    kind: CityBuildingKind,
    display_name: &'static str,
    scene_path: &'static str,
    building_type: BuildingType,
    recommended_zone: PlotZone,
    recommended_archetype: PlotArchetype,
    footprint: Vec2,
    height: f32,
    side_yard: f32,
    rear_yard: f32,
    recommended_setback: f32,
    recommended_spacing: f32,
    local_center: Vec2,
    base_y: f32,
    front_axis: BuildingFrontAxis,
    visual_yaw_offset_degrees: f32,
) -> CityBuildingSpec {
    CityBuildingSpec {
        kind,
        display_name,
        scene_path,
        building_type,
        recommended_zone,
        recommended_archetype,
        footprint,
        height,
        side_yard,
        rear_yard,
        recommended_setback,
        recommended_spacing,
        local_center,
        base_y,
        front_axis,
        visual_yaw_offset_degrees,
    }
}

pub fn suggested_plot_half_extents(kind: CityBuildingKind) -> Vec2 {
    let spec = kind.spec();
    Vec2::new(
        spec.footprint.x * 0.5 + spec.side_yard,
        spec.footprint.y * 0.5 + spec.rear_yard,
    )
}

pub fn plot_building_rotation_y(plot: &MapPlot) -> f32 {
    let rotation_y = plot.rotation_degrees.to_radians();
    rotation_y
}

pub fn plot_toward_road_direction(plot: &MapPlot) -> Vec2 {
    let rotation_y = plot.rotation_degrees.to_radians();
    let road_normal = Vec2::new(-rotation_y.sin(), rotation_y.cos());
    match plot.driveway_side.unwrap_or(super::RoadSide::Left) {
        super::RoadSide::Left => -road_normal,
        super::RoadSide::Right => road_normal,
    }
}

pub fn plot_building_front_direction(plot: &MapPlot, kind: CityBuildingKind) -> Vec2 {
    let toward_road = plot_toward_road_direction(plot);
    let spec = kind.spec();
    match spec.front_axis {
        BuildingFrontAxis::PositiveZ | BuildingFrontAxis::NegativeZ => toward_road,
    }
}

pub fn plot_building_world_rotation_y(plot: &MapPlot, kind: CityBuildingKind) -> f32 {
    let base_rotation = plot_building_rotation_y(plot);
    let spec = kind.spec();
    let side = plot.driveway_side.unwrap_or(super::RoadSide::Left);
    let facing_rotation = match (spec.front_axis, side) {
        (BuildingFrontAxis::PositiveZ, super::RoadSide::Left) => base_rotation + PI,
        (BuildingFrontAxis::PositiveZ, super::RoadSide::Right) => base_rotation,
        (BuildingFrontAxis::NegativeZ, super::RoadSide::Left) => base_rotation,
        (BuildingFrontAxis::NegativeZ, super::RoadSide::Right) => base_rotation + PI,
    };
    facing_rotation + spec.visual_yaw_offset_degrees.to_radians()
}

pub fn plot_building_rect(plot: &MapPlot, kind: CityBuildingKind) -> OrientedRect {
    let spec = kind.spec();
    let rotation_y = plot_building_world_rotation_y(plot, kind);
    let toward_road = plot_toward_road_direction(plot);
    let plot_half_depth = plot.half_extents_vec2().y;
    let building_half_depth = spec.footprint.y * 0.5;
    let bias = (plot_half_depth - building_half_depth).max(0.0);

    OrientedRect {
        center: plot.center_vec2() + toward_road * bias,
        half_extents: spec.footprint * 0.5,
        rotation_y,
    }
}

pub fn plot_building_ground_position(
    plot: &MapPlot,
    kind: CityBuildingKind,
    ground_y: f32,
) -> Vec3 {
    let rect = plot_building_rect(plot, kind);
    Vec3::new(rect.center.x, ground_y, rect.center.y)
}

pub fn plot_building_scene_transform(
    plot: &MapPlot,
    kind: CityBuildingKind,
    ground_y: f32,
) -> Transform {
    let spec = kind.spec();
    let rect = plot_building_rect(plot, kind);
    let axis_x = Vec2::new(rect.rotation_y.cos(), rect.rotation_y.sin());
    let axis_z = Vec2::new(-rect.rotation_y.sin(), rect.rotation_y.cos());
    let root_xz = rect.center - axis_x * spec.local_center.x - axis_z * spec.local_center.y;

    Transform::from_translation(Vec3::new(root_xz.x, ground_y - spec.base_y, root_xz.y))
        .with_rotation(building_rotation_quat(rect.rotation_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::RoadSide;

    #[test]
    fn suggested_plot_half_extents_include_padding() {
        let spec = CityBuildingKind::Farmstead.spec();
        let suggested = suggested_plot_half_extents(CityBuildingKind::Farmstead);
        assert!(suggested.x > spec.footprint.x * 0.5);
        assert!(suggested.y > spec.footprint.y * 0.5);
    }

    #[test]
    fn left_side_plots_bias_buildings_toward_the_road() {
        let plot = MapPlot {
            id: 1,
            center: [20.0, 10.0],
            half_extents: [8.0, 10.0],
            rotation_degrees: 0.0,
            zone: PlotZone::Residential,
            frontage_road_id: Some(5),
            setback: 4.0,
            driveway_side: Some(RoadSide::Left),
            archetypes: Vec::new(),
            building_kind: Some(CityBuildingKind::LogCabin),
            tags: Vec::new(),
        };

        let rect = plot_building_rect(&plot, CityBuildingKind::LogCabin);
        assert!(rect.rotation_y > PI * 0.9);
        assert!(rect.center.y < plot.center[1]);
        let front = plot_building_front_direction(&plot, CityBuildingKind::LogCabin);
        assert!(front.y < 0.0);
    }

    #[test]
    fn right_side_plots_flip_building_to_face_the_road() {
        let plot = MapPlot {
            id: 2,
            center: [20.0, -10.0],
            half_extents: [8.0, 10.0],
            rotation_degrees: 0.0,
            zone: PlotZone::Residential,
            frontage_road_id: Some(5),
            setback: 4.0,
            driveway_side: Some(RoadSide::Right),
            archetypes: Vec::new(),
            building_kind: Some(CityBuildingKind::LogCabin),
            tags: Vec::new(),
        };

        let rect = plot_building_rect(&plot, CityBuildingKind::LogCabin);
        assert!(rect.rotation_y.abs() < 1.0e-4);
        assert!(rect.center.y > plot.center[1]);
        let front = plot_building_front_direction(&plot, CityBuildingKind::LogCabin);
        assert!(front.y > 0.0);
    }

    #[test]
    fn scene_transform_forward_matches_frontage_math_for_left_side_plot() {
        let plot = MapPlot {
            id: 3,
            center: [12.0, 18.0],
            half_extents: [8.0, 10.0],
            rotation_degrees: 90.0,
            zone: PlotZone::Residential,
            frontage_road_id: Some(7),
            setback: 4.0,
            driveway_side: Some(RoadSide::Left),
            archetypes: Vec::new(),
            building_kind: Some(CityBuildingKind::LogCabin),
            tags: Vec::new(),
        };

        let transform = plot_building_scene_transform(&plot, CityBuildingKind::LogCabin, 0.0);
        let forward = transform.rotation * Vec3::Z;
        let scene_forward = Vec2::new(forward.x, forward.z).normalize_or_zero();
        let frontage_forward =
            plot_building_front_direction(&plot, CityBuildingKind::LogCabin).normalize_or_zero();

        assert!(scene_forward.distance(frontage_forward) < 1.0e-4);
    }

    #[test]
    fn scene_transform_forward_matches_frontage_math_for_right_side_plot() {
        let plot = MapPlot {
            id: 4,
            center: [12.0, 18.0],
            half_extents: [8.0, 10.0],
            rotation_degrees: 32.0,
            zone: PlotZone::Residential,
            frontage_road_id: Some(7),
            setback: 4.0,
            driveway_side: Some(RoadSide::Right),
            archetypes: Vec::new(),
            building_kind: Some(CityBuildingKind::LogCabin),
            tags: Vec::new(),
        };

        let transform = plot_building_scene_transform(&plot, CityBuildingKind::LogCabin, 0.0);
        let forward = transform.rotation * Vec3::Z;
        let scene_forward = Vec2::new(forward.x, forward.z).normalize_or_zero();
        let frontage_forward =
            plot_building_front_direction(&plot, CityBuildingKind::LogCabin).normalize_or_zero();

        assert!(scene_forward.distance(frontage_forward) < 1.0e-4);
    }
}
