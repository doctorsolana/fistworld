//! masks systems.

use super::*;

pub(super) fn paint_mask(op: &TerrainPaintOp, point: Vec2) -> f32 {
    match op.shape {
        shared::terrain::TerrainPaintShape::Rect {
            center,
            half_extents,
            rotation,
        } => {
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let rel = point - center;
            let local = Vec2::new(
                rel.x * cos_r + rel.y * sin_r,
                -rel.x * sin_r + rel.y * cos_r,
            );
            let dx = local.x.abs() - half_extents.x;
            let dz = local.y.abs() - half_extents.y;
            let dist = dx.max(dz);
            if dist <= 0.0 {
                1.0
            } else if dist >= op.falloff {
                0.0
            } else {
                1.0 - smoothstep(0.0, op.falloff, dist)
            }
        }
        shared::terrain::TerrainPaintShape::Line { start, end, width } => {
            let dist = distance_point_to_segment(point, start, end) - width * 0.5;
            if dist <= 0.0 {
                1.0
            } else if dist >= op.falloff {
                0.0
            } else {
                1.0 - smoothstep(0.0, op.falloff, dist)
            }
        }
        shared::terrain::TerrainPaintShape::Circle { center, radius } => {
            let dist = point.distance(center) - radius;
            if dist <= 0.0 {
                1.0
            } else if dist >= op.falloff {
                0.0
            } else {
                1.0 - smoothstep(0.0, op.falloff, dist)
            }
        }
    }
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return 0.0;
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn distance_point_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let ab = end - start;
    let ab_len_sq = ab.length_squared();
    if ab_len_sq <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(ab) / ab_len_sq).clamp(0.0, 1.0);
    let closest = start + ab * t;
    point.distance(closest)
}
