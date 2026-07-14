use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{ChunkCoord, TerrainGenerator, CHUNK_SIZE};

pub const TERRAIN_WEIGHTMAP_RESOLUTION: u32 = 64;

/// Terrain surface layers used for splatmap blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainLayer {
    Grass,
    Dirt,
    Sand,
    Cobblestone,
}

impl TerrainLayer {
    pub fn index(self) -> usize {
        match self {
            TerrainLayer::Grass => 0,
            TerrainLayer::Dirt => 1,
            TerrainLayer::Sand => 2,
            TerrainLayer::Cobblestone => 3,
        }
    }
}

/// Shape used for terrain paint operations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TerrainPaintShape {
    Rect {
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
    },
    Line {
        start: Vec2,
        end: Vec2,
        width: f32,
    },
    Circle {
        center: Vec2,
        radius: f32,
    },
}

/// Server-authoritative terrain paint operation (replicated).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainPaintOp {
    pub id: u64,
    pub layer: TerrainLayer,
    /// Strength 0..1 applied to the target layer.
    pub strength: f32,
    /// Blend falloff width in meters (soft edge).
    pub falloff: f32,
    pub shape: TerrainPaintShape,
}

impl TerrainPaintOp {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0 {
            return Err("id must be greater than zero".to_string());
        }
        if !self.strength.is_finite() || !(0.0..=1.0).contains(&self.strength) {
            return Err("strength must be finite and between 0 and 1".to_string());
        }
        if !self.falloff.is_finite() || self.falloff < 0.0 {
            return Err("falloff must be finite and non-negative".to_string());
        }

        match self.shape {
            TerrainPaintShape::Rect {
                center,
                half_extents,
                rotation,
            } => {
                validate_vec2(center, "rect center")?;
                validate_vec2(half_extents, "rect half_extents")?;
                if half_extents.x <= 0.0 || half_extents.y <= 0.0 {
                    return Err("rect half_extents must be positive".to_string());
                }
                if !rotation.is_finite() {
                    return Err("rect rotation must be finite".to_string());
                }
            }
            TerrainPaintShape::Line { start, end, width } => {
                validate_vec2(start, "line start")?;
                validate_vec2(end, "line end")?;
                if !width.is_finite() || width <= 0.0 {
                    return Err("line width must be finite and positive".to_string());
                }
            }
            TerrainPaintShape::Circle { center, radius } => {
                validate_vec2(center, "circle center")?;
                if !radius.is_finite() || radius <= 0.0 {
                    return Err("circle radius must be finite and positive".to_string());
                }
            }
        }

        Ok(())
    }
}

pub fn terrain_paint_op_bounds(op: &TerrainPaintOp) -> (Vec2, Vec2) {
    match op.shape {
        TerrainPaintShape::Rect {
            center,
            half_extents,
            rotation,
        } => {
            let ext = half_extents + Vec2::splat(op.falloff);
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let corners = [
                Vec2::new(-ext.x, -ext.y),
                Vec2::new(ext.x, -ext.y),
                Vec2::new(-ext.x, ext.y),
                Vec2::new(ext.x, ext.y),
            ];
            let mut min = Vec2::splat(f32::MAX);
            let mut max = Vec2::splat(f32::MIN);
            for corner in corners {
                let world = Vec2::new(
                    center.x + corner.x * cos_r - corner.y * sin_r,
                    center.y + corner.x * sin_r + corner.y * cos_r,
                );
                min = min.min(world);
                max = max.max(world);
            }
            (min, max)
        }
        TerrainPaintShape::Line { start, end, width } => {
            let extra = width * 0.5 + op.falloff;
            (
                start.min(end) - Vec2::splat(extra),
                start.max(end) + Vec2::splat(extra),
            )
        }
        TerrainPaintShape::Circle { center, radius } => {
            let ext = Vec2::splat(radius + op.falloff);
            (center - ext, center + ext)
        }
    }
}

pub fn terrain_paint_op_intersects_chunk(
    op: &TerrainPaintOp,
    chunk_min: Vec2,
    chunk_max: Vec2,
) -> bool {
    let (min, max) = terrain_paint_op_bounds(op);
    max.x >= chunk_min.x && min.x <= chunk_max.x && max.y >= chunk_min.y && min.y <= chunk_max.y
}

pub fn terrain_paint_op_chunk_coords(op: &TerrainPaintOp) -> Vec<ChunkCoord> {
    let (min, max) = terrain_paint_op_bounds(op);
    let min_chunk_x = (min.x / CHUNK_SIZE).floor() as i32;
    let max_chunk_x = (max.x / CHUNK_SIZE).floor() as i32;
    let min_chunk_z = (min.y / CHUNK_SIZE).floor() as i32;
    let max_chunk_z = (max.y / CHUNK_SIZE).floor() as i32;

    let mut coords = Vec::new();
    for x in min_chunk_x..=max_chunk_x {
        for z in min_chunk_z..=max_chunk_z {
            coords.push(ChunkCoord::new(x, z));
        }
    }
    coords
}

pub fn build_terrain_weightmap_weights(
    generator: &TerrainGenerator,
    coord: ChunkCoord,
    ops: &[TerrainPaintOp],
    resolution: u32,
) -> Vec<[u8; 4]> {
    let mut weights = vec![[0u8; 4]; (resolution * resolution) as usize];
    let origin = coord.world_pos();
    let step = CHUNK_SIZE / resolution as f32;

    for z in 0..resolution {
        for x in 0..resolution {
            let world_x = origin.x + (x as f32 + 0.5) * step;
            let world_z = origin.z + (z as f32 + 0.5) * step;
            weights[(z * resolution + x) as usize] =
                weights_to_bytes(generator.get_surface_weights(world_x, world_z));
        }
    }

    let chunk_min = Vec2::new(origin.x, origin.z);
    let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);
    let mut relevant_ops: Vec<&TerrainPaintOp> = ops
        .iter()
        .filter(|op| terrain_paint_op_intersects_chunk(op, chunk_min, chunk_max))
        .collect();
    relevant_ops.sort_unstable_by_key(|op| op.id);
    for op in relevant_ops {
        apply_terrain_paint_op_to_weights(op, chunk_min, &mut weights, resolution);
    }

    weights
}

pub fn apply_terrain_paint_op_to_weights(
    op: &TerrainPaintOp,
    chunk_min: Vec2,
    weights: &mut [[u8; 4]],
    resolution: u32,
) {
    let step = CHUNK_SIZE / resolution as f32;
    let (min, max) = terrain_paint_op_bounds(op);
    let last = resolution.saturating_sub(1) as f32;
    let min_x = ((min.x - chunk_min.x) / CHUNK_SIZE * resolution as f32)
        .floor()
        .clamp(0.0, last) as u32;
    let max_x = ((max.x - chunk_min.x) / CHUNK_SIZE * resolution as f32)
        .ceil()
        .clamp(0.0, last) as u32;
    let min_z = ((min.y - chunk_min.y) / CHUNK_SIZE * resolution as f32)
        .floor()
        .clamp(0.0, last) as u32;
    let max_z = ((max.y - chunk_min.y) / CHUNK_SIZE * resolution as f32)
        .ceil()
        .clamp(0.0, last) as u32;

    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let point = Vec2::new(
                chunk_min.x + (x as f32 + 0.5) * step,
                chunk_min.y + (z as f32 + 0.5) * step,
            );
            let amount = terrain_paint_mask(op, point) * op.strength;
            if amount > 0.0 {
                apply_layer_weight(
                    &mut weights[(z * resolution + x) as usize],
                    op.layer,
                    amount,
                );
            }
        }
    }
}

fn terrain_paint_mask(op: &TerrainPaintOp, point: Vec2) -> f32 {
    let distance = match op.shape {
        TerrainPaintShape::Rect {
            center,
            half_extents,
            rotation,
        } => {
            let (sin_r, cos_r) = rotation.sin_cos();
            let rel = point - center;
            let local = Vec2::new(
                rel.x * cos_r + rel.y * sin_r,
                -rel.x * sin_r + rel.y * cos_r,
            );
            (local.abs() - half_extents).max_element()
        }
        TerrainPaintShape::Line { start, end, width } => {
            distance_point_to_segment(point, start, end) - width * 0.5
        }
        TerrainPaintShape::Circle { center, radius } => point.distance(center) - radius,
    };

    if distance <= 0.0 {
        1.0
    } else if op.falloff <= 0.0 || distance >= op.falloff {
        0.0
    } else {
        1.0 - smoothstep(0.0, op.falloff, distance)
    }
}

fn apply_layer_weight(weights: &mut [u8; 4], layer: TerrainLayer, amount: f32) {
    let amount = amount.clamp(0.0, 1.0);
    let mut values = [
        weights[0] as f32 / 255.0,
        weights[1] as f32 / 255.0,
        weights[2] as f32 / 255.0,
        weights[3] as f32 / 255.0,
    ];
    let target = layer.index();
    for (index, value) in values.iter_mut().enumerate() {
        if index == target {
            *value += amount * (1.0 - *value);
        } else {
            *value *= 1.0 - amount;
        }
    }
    *weights = weights_to_bytes(values);
}

fn weights_to_bytes(weights: [f32; 4]) -> [u8; 4] {
    let mut normalized = weights.map(|value| value.max(0.0));
    let sum: f32 = normalized.iter().sum();
    if sum <= f32::EPSILON {
        return [255, 0, 0, 0];
    }
    for value in &mut normalized {
        *value /= sum;
    }

    let mut bytes = normalized.map(|value| (value * 255.0).round().clamp(0.0, 255.0) as u8);
    let total: i32 = bytes.iter().map(|value| *value as i32).sum();
    let difference = 255 - total;
    if difference != 0 {
        let largest = bytes
            .iter()
            .enumerate()
            .max_by_key(|(_, value)| **value)
            .map(|(index, _)| index)
            .unwrap_or(0);
        bytes[largest] = (bytes[largest] as i32 + difference).clamp(0, 255) as u8;
    }
    bytes
}

fn distance_point_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn validate_vec2(value: Vec2, label: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{label} must be finite"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_paint_increases_the_selected_layer_and_preserves_normalization() {
        let op = TerrainPaintOp {
            id: 1,
            layer: TerrainLayer::Cobblestone,
            strength: 0.75,
            falloff: 0.0,
            shape: TerrainPaintShape::Circle {
                center: Vec2::splat(CHUNK_SIZE * 0.5),
                radius: CHUNK_SIZE,
            },
        };
        let mut weights = vec![[255, 0, 0, 0]; 16];

        apply_terrain_paint_op_to_weights(&op, Vec2::ZERO, &mut weights, 4);

        assert!(weights.iter().all(|weight| weight[3] > weight[0]));
        assert!(weights
            .iter()
            .all(|weight| weight.iter().map(|value| *value as u16).sum::<u16>() == 255));
    }

    #[test]
    fn invalid_strength_is_rejected() {
        let op = TerrainPaintOp {
            id: 1,
            layer: TerrainLayer::Grass,
            strength: 1.1,
            falloff: 1.0,
            shape: TerrainPaintShape::Circle {
                center: Vec2::ZERO,
                radius: 2.0,
            },
        };

        assert!(op.validate().is_err());
    }
}
