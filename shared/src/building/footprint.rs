use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::building::BuildingType;

/// Component for placed buildings (replicated).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedBuilding {
    pub building_type: BuildingType,
    /// Rotation in radians (Y-axis rotation).
    pub rotation: f32,
}

/// Network position for placed buildings.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingPosition(pub Vec3);

/// Message from client to request placing a building.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlaceBuildingRequest {
    pub building_type: BuildingType,
    pub position: Vec3,
    /// Rotation in radians (Y-axis).
    pub rotation: f32,
}

/// Check if a point (XZ plane) is inside a rotated rectangle.
pub fn point_in_rotated_rect(
    point_xz: Vec2,
    center_xz: Vec2,
    half_extents: Vec2,
    rotation_y: f32,
) -> bool {
    let rel = point_xz - center_xz;
    let cos_r = rotation_y.cos();
    let sin_r = rotation_y.sin();

    let local_x = rel.x * cos_r + rel.y * sin_r;
    let local_z = -rel.x * sin_r + rel.y * cos_r;

    local_x.abs() <= half_extents.x && local_z.abs() <= half_extents.y
}

/// Check if two rotated rectangles overlap (SAT in 2D).
pub fn rotated_rects_overlap(
    center_a: Vec2,
    half_a: Vec2,
    rot_a: f32,
    center_b: Vec2,
    half_b: Vec2,
    rot_b: f32,
) -> bool {
    let (ax_a, az_a) = rect_axes(rot_a);
    let (ax_b, az_b) = rect_axes(rot_b);
    let axes = [ax_a, az_a, ax_b, az_b];
    let eps = 1e-4;

    for axis in axes {
        let axis = axis.normalize_or_zero();
        if axis.length_squared() < 1e-6 {
            continue;
        }
        let ra = half_a.x * ax_a.dot(axis).abs() + half_a.y * az_a.dot(axis).abs();
        let rb = half_b.x * ax_b.dot(axis).abs() + half_b.y * az_b.dot(axis).abs();
        let dist = (center_a.dot(axis) - center_b.dot(axis)).abs();
        if dist > ra + rb + eps {
            return false;
        }
    }
    true
}

fn rect_axes(rotation_y: f32) -> (Vec2, Vec2) {
    let cos_r = rotation_y.cos();
    let sin_r = rotation_y.sin();
    let axis_x = Vec2::new(cos_r, sin_r);
    let axis_z = Vec2::new(-sin_r, cos_r);
    (axis_x, axis_z)
}
