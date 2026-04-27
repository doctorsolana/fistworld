use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::CityBuildingKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RoadClass {
    Alley,
    #[default]
    Local,
    Collector,
    Arterial,
}

impl RoadClass {
    pub const fn default_lane_count(self) -> u8 {
        match self {
            RoadClass::Alley => 1,
            RoadClass::Local => 2,
            RoadClass::Collector => 2,
            RoadClass::Arterial => 4,
        }
    }

    pub const fn default_width(self) -> f32 {
        match self {
            RoadClass::Alley => 4.5,
            RoadClass::Local => 8.0,
            RoadClass::Collector => 12.0,
            RoadClass::Arterial => 18.0,
        }
    }

    pub const fn default_sidewalk_width(self) -> f32 {
        match self {
            RoadClass::Alley => 1.0,
            RoadClass::Local => 2.0,
            RoadClass::Collector => 2.5,
            RoadClass::Arterial => 3.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlotZone {
    #[default]
    Residential,
    MixedUse,
    Commercial,
    Industrial,
    Civic,
    Park,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoadSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlotArchetype {
    #[default]
    HouseSmall,
    HouseRow,
    CornerStore,
    ApartmentLowrise,
    Warehouse,
    Civic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapRoad {
    pub id: u64,
    pub points: Vec<[f32; 2]>,
    #[serde(default = "default_road_width")]
    pub width: f32,
    #[serde(default)]
    pub road_class: RoadClass,
    #[serde(default = "default_lane_count")]
    pub lane_count: u8,
    #[serde(default = "default_has_sidewalk")]
    pub sidewalk_left: bool,
    #[serde(default = "default_has_sidewalk")]
    pub sidewalk_right: bool,
    #[serde(default = "default_sidewalk_width")]
    pub sidewalk_width: f32,
    #[serde(default)]
    pub parking_left: bool,
    #[serde(default)]
    pub parking_right: bool,
    #[serde(default)]
    pub district: Option<String>,
}

impl MapRoad {
    pub fn validate(&self) -> Result<(), String> {
        if self.points.len() < 2 {
            return Err("requires at least 2 points".to_string());
        }
        if self.width <= 0.0 {
            return Err("width must be > 0".to_string());
        }
        if self.lane_count == 0 {
            return Err("lane_count must be > 0".to_string());
        }
        if self.sidewalk_width < 0.0 {
            return Err("sidewalk_width must be >= 0".to_string());
        }
        Ok(())
    }

    pub fn total_half_width(&self) -> f32 {
        let sidewalk = if self.sidewalk_left || self.sidewalk_right {
            self.sidewalk_width.max(0.0)
        } else {
            0.0
        };
        self.width * 0.5 + sidewalk
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPlot {
    pub id: u64,
    pub center: [f32; 2],
    pub half_extents: [f32; 2],
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default)]
    pub zone: PlotZone,
    #[serde(default)]
    pub frontage_road_id: Option<u64>,
    #[serde(default = "default_plot_setback")]
    pub setback: f32,
    #[serde(default)]
    pub driveway_side: Option<RoadSide>,
    #[serde(default)]
    pub archetypes: Vec<PlotArchetype>,
    #[serde(default)]
    pub building_kind: Option<CityBuildingKind>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl MapPlot {
    pub fn validate(&self) -> Result<(), String> {
        if self.half_extents[0] <= 0.0 || self.half_extents[1] <= 0.0 {
            return Err("half_extents must be > 0".to_string());
        }
        if self.setback < 0.0 {
            return Err("setback must be >= 0".to_string());
        }
        Ok(())
    }

    #[inline]
    pub fn center_vec2(&self) -> Vec2 {
        Vec2::new(self.center[0], self.center[1])
    }

    #[inline]
    pub fn half_extents_vec2(&self) -> Vec2 {
        Vec2::new(self.half_extents[0], self.half_extents[1])
    }
}

#[inline]
fn default_road_width() -> f32 {
    RoadClass::Local.default_width()
}

#[inline]
fn default_lane_count() -> u8 {
    RoadClass::Local.default_lane_count()
}

#[inline]
fn default_has_sidewalk() -> bool {
    true
}

#[inline]
fn default_sidewalk_width() -> f32 {
    RoadClass::Local.default_sidewalk_width()
}

#[inline]
fn default_plot_setback() -> f32 {
    3.0
}
