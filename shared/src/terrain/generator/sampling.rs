use std::collections::HashMap;

use bevy::prelude::*;

use crate::terrain::TerrainDeltaData;

use super::{ChunkCoord, CHUNK_RESOLUTION, VERTEX_SPACING};

pub(super) fn sample_delta_from_map(
    delta_chunks: &HashMap<ChunkCoord, TerrainDeltaData>,
    x: f32,
    z: f32,
) -> f32 {
    let chunk_coord = ChunkCoord::from_world_pos(Vec3::new(x, 0.0, z));
    let Some(delta_data) = delta_chunks.get(&chunk_coord) else {
        return 0.0;
    };

    let chunk_origin = chunk_coord.world_pos();
    let local_x = x - chunk_origin.x;
    let local_z = z - chunk_origin.z;

    let grid_x = local_x / VERTEX_SPACING;
    let grid_z = local_z / VERTEX_SPACING;

    let xi = grid_x.floor() as i32;
    let zi = grid_z.floor() as i32;
    let fx = grid_x - xi as f32;
    let fz = grid_z - zi as f32;

    let xi = xi.clamp(0, CHUNK_RESOLUTION as i32 - 2) as usize;
    let zi = zi.clamp(0, CHUNK_RESOLUTION as i32 - 2) as usize;

    let d00 = delta_data.get_vertex(xi, zi);
    let d10 = delta_data.get_vertex(xi + 1, zi);
    let d01 = delta_data.get_vertex(xi, zi + 1);
    let d11 = delta_data.get_vertex(xi + 1, zi + 1);

    let dx0 = d00 + fx * (d10 - d00);
    let dx1 = d01 + fx * (d11 - d01);
    dx0 + fz * (dx1 - dx0)
}
