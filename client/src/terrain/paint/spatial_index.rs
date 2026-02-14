//! spatial index systems.

use super::*;

pub(super) fn paint_op_bounds(op: &TerrainPaintOp) -> (Vec2, Vec2) {
    match op.shape {
        shared::terrain::TerrainPaintShape::Rect {
            center,
            half_extents,
            rotation,
        } => {
            let extra = op.falloff;
            let ext = half_extents + Vec2::splat(extra);
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
            for corner in corners.iter() {
                let world = Vec2::new(
                    center.x + corner.x * cos_r - corner.y * sin_r,
                    center.y + corner.x * sin_r + corner.y * cos_r,
                );
                min.x = min.x.min(world.x);
                min.y = min.y.min(world.y);
                max.x = max.x.max(world.x);
                max.y = max.y.max(world.y);
            }
            (min, max)
        }
        shared::terrain::TerrainPaintShape::Line { start, end, width } => {
            let extra = width * 0.5 + op.falloff;
            let min = Vec2::new(start.x.min(end.x) - extra, start.y.min(end.y) - extra);
            let max = Vec2::new(start.x.max(end.x) + extra, start.y.max(end.y) + extra);
            (min, max)
        }
        shared::terrain::TerrainPaintShape::Circle { center, radius } => {
            let r = radius + op.falloff;
            let min = Vec2::new(center.x - r, center.y - r);
            let max = Vec2::new(center.x + r, center.y + r);
            (min, max)
        }
    }
}

pub(super) fn op_chunk_coords(op: &TerrainPaintOp) -> Vec<ChunkCoord> {
    let (min, max) = paint_op_bounds(op);
    let min_chunk_x = (min.x / CHUNK_SIZE).floor() as i32;
    let max_chunk_x = (max.x / CHUNK_SIZE).floor() as i32;
    let min_chunk_z = (min.y / CHUNK_SIZE).floor() as i32;
    let max_chunk_z = (max.y / CHUNK_SIZE).floor() as i32;

    let mut coords = Vec::new();
    for cx in min_chunk_x..=max_chunk_x {
        for cz in min_chunk_z..=max_chunk_z {
            coords.push(ChunkCoord::new(cx, cz));
        }
    }
    coords
}
