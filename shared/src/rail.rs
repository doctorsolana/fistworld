use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::economy::{CargoAmount, CargoKind, EconomyInventory, IndustryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component, Default)]
pub struct CompanyId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component, Default)]
pub struct TrackSegmentId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component, Default)]
pub struct StationId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component, Default)]
pub struct TrainId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component, Default)]
pub struct IndustryId(pub u64);

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Company {
    pub id: CompanyId,
    pub owner_peer: u64,
    pub name: String,
    pub color: [f32; 4],
    pub money: i64,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompanyLedger {
    pub company: CompanyId,
    pub lifetime_revenue: i64,
    pub lifetime_construction_spend: i64,
    pub last_delivery_revenue: i64,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RailTrackSegment {
    pub id: TrackSegmentId,
    pub owner: CompanyId,
    pub start: Vec3,
    pub control_a: Vec3,
    pub control_b: Vec3,
    pub end: Vec3,
    pub length: f32,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RailStation {
    pub id: StationId,
    pub owner: CompanyId,
    pub name: String,
    pub position: Vec3,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Train {
    pub id: TrainId,
    pub owner: CompanyId,
    pub cargo_policy: Option<CargoKind>,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainState {
    pub position: Vec3,
    pub target_stop_index: usize,
    pub speed_mps: f32,
}

impl TrainState {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            target_stop_index: 0,
            speed_mps: 18.0,
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainRoute {
    pub train: TrainId,
    pub stops: Vec<RouteStop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RouteStop {
    pub station: StationId,
    pub cargo: Option<CargoKind>,
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Industry {
    pub id: IndustryId,
    pub kind: IndustryKind,
    pub name: String,
    pub position: Vec3,
    pub inventory: EconomyInventory,
}

impl Industry {
    pub fn new(
        id: IndustryId,
        kind: IndustryKind,
        name: impl Into<String>,
        position: Vec3,
    ) -> Self {
        let inventory = kind
            .primary_output()
            .map(|cargo| EconomyInventory {
                cargo: vec![CargoAmount::new(cargo, 20.0)],
            })
            .unwrap_or_default();
        Self {
            id,
            kind,
            name: name.into(),
            position,
            inventory,
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Town {
    pub id: IndustryId,
    pub name: String,
    pub position: Vec3,
    pub population: u32,
}

pub const MAX_TRACK_GRADE: f32 = 0.10;
pub const MIN_TRACK_LENGTH: f32 = 6.0;
pub const MAX_TRACK_LENGTH: f32 = 450.0;
pub const STATION_SNAP_RADIUS: f32 = 48.0;

pub fn cubic_bezier_point(
    start: Vec3,
    control_a: Vec3,
    control_b: Vec3,
    end: Vec3,
    t: f32,
) -> Vec3 {
    let u = 1.0 - t;
    start * (u * u * u)
        + control_a * (3.0 * u * u * t)
        + control_b * (3.0 * u * t * t)
        + end * (t * t * t)
}

pub fn approximate_bezier_length(
    start: Vec3,
    control_a: Vec3,
    control_b: Vec3,
    end: Vec3,
    samples: usize,
) -> f32 {
    let samples = samples.max(1);
    let mut length = 0.0;
    let mut previous = start;
    for i in 1..=samples {
        let t = i as f32 / samples as f32;
        let point = cubic_bezier_point(start, control_a, control_b, end, t);
        length += previous.distance(point);
        previous = point;
    }
    length
}

pub fn max_sampled_grade(start: Vec3, control_a: Vec3, control_b: Vec3, end: Vec3) -> f32 {
    let mut max_grade = 0.0f32;
    let mut previous = start;
    for i in 1..=16 {
        let point = cubic_bezier_point(start, control_a, control_b, end, i as f32 / 16.0);
        let horizontal = Vec2::new(point.x - previous.x, point.z - previous.z).length();
        if horizontal > 0.001 {
            max_grade = max_grade.max((point.y - previous.y).abs() / horizontal);
        }
        previous = point;
    }
    max_grade
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_bezier_length_matches_distance() {
        let start = Vec3::ZERO;
        let end = Vec3::new(100.0, 0.0, 0.0);
        let control_a = Vec3::new(33.0, 0.0, 0.0);
        let control_b = Vec3::new(66.0, 0.0, 0.0);
        let length = approximate_bezier_length(start, control_a, control_b, end, 16);
        assert!((length - 100.0).abs() < 0.01);
    }

    #[test]
    fn grade_validation_samples_curve() {
        let start = Vec3::ZERO;
        let end = Vec3::new(100.0, 15.0, 0.0);
        let control_a = Vec3::new(33.0, 5.0, 0.0);
        let control_b = Vec3::new(66.0, 10.0, 0.0);
        assert!(max_sampled_grade(start, control_a, control_b, end) > MAX_TRACK_GRADE);
    }
}
