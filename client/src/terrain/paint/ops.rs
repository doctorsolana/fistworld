//! ops systems.

use super::*;

pub(crate) fn paint_op_intersects_chunk(
    op: &TerrainPaintOp,
    chunk_min: Vec2,
    chunk_max: Vec2,
) -> bool {
    let (min, max) = paint_op_bounds(op);
    max.x >= chunk_min.x && min.x <= chunk_max.x && max.y >= chunk_min.y && min.y <= chunk_max.y
}

pub(crate) fn apply_paint_op_to_weights(
    op: &TerrainPaintOp,
    chunk_min: Vec2,
    weights: &mut [[u8; 4]],
    resolution: u32,
) {
    let step = CHUNK_SIZE / resolution as f32;
    let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);

    // Compute AABB for op to limit iteration.
    let (min, max) = paint_op_bounds(op);

    let min_x = ((min.x - chunk_min.x) / CHUNK_SIZE * resolution as f32)
        .floor()
        .clamp(0.0, (resolution - 1) as f32) as u32;
    let max_x = ((max.x - chunk_min.x) / CHUNK_SIZE * resolution as f32)
        .ceil()
        .clamp(0.0, (resolution - 1) as f32) as u32;
    let min_z = ((min.y - chunk_min.y) / CHUNK_SIZE * resolution as f32)
        .floor()
        .clamp(0.0, (resolution - 1) as f32) as u32;
    let max_z = ((max.y - chunk_min.y) / CHUNK_SIZE * resolution as f32)
        .ceil()
        .clamp(0.0, (resolution - 1) as f32) as u32;

    for zi in min_z..=max_z {
        for xi in min_x..=max_x {
            let world_x = chunk_min.x + (xi as f32 + 0.5) * step;
            let world_z = chunk_min.y + (zi as f32 + 0.5) * step;
            if world_x < chunk_min.x
                || world_x > chunk_max.x
                || world_z < chunk_min.y
                || world_z > chunk_max.y
            {
                continue;
            }
            let mask = paint_mask(op, Vec2::new(world_x, world_z));
            if mask <= 0.0 {
                continue;
            }
            let idx = (zi * resolution + xi) as usize;
            apply_layer_weight(&mut weights[idx], op.layer, mask * op.strength);
        }
    }
}

/// Ingest replicated TerrainPaintOp components into local paint state.
pub(crate) fn ingest_paint_ops(
    mut paint_state: ResMut<TerrainPaintState>,
    mut paint_index: ResMut<TerrainPaintSpatialIndex>,
    new_ops: Query<&TerrainPaintOp, Added<TerrainPaintOp>>,
    loaded_chunks: Res<LoadedChunks>,
    mut images: ResMut<Assets<Image>>,
    mut perf: ResMut<PerfHitchStats>,
    debug_perf: Res<DebugPerfSettings>,
) {
    let start = Instant::now();
    let mut added_ops: Vec<TerrainPaintOp> = Vec::new();
    let mut ops_by_loaded_chunk: HashMap<ChunkCoord, Vec<u64>> = HashMap::new();
    let mut chunks_updated = 0u32;

    for op in new_ops.iter() {
        if paint_state.ops.contains_key(&op.id) {
            continue;
        }
        paint_state.ops.insert(op.id, op.clone());
        added_ops.push(op.clone());

        for coord in op_chunk_coords(op) {
            if !coord.in_world_bounds() {
                continue;
            }
            let ids = paint_index.ops_by_chunk.entry(coord).or_default();
            if !ids.contains(&op.id) {
                ids.push(op.id);
            }
            if loaded_chunks.chunks.contains(&coord) {
                ops_by_loaded_chunk.entry(coord).or_default().push(op.id);
            }
        }
    }

    for (coord, op_ids) in ops_by_loaded_chunk.into_iter() {
        let mut ops_for_chunk: Vec<TerrainPaintOp> = Vec::new();
        for op_id in op_ids {
            if let Some(op) = paint_state.ops.get(&op_id) {
                ops_for_chunk.push(op.clone());
            }
        }
        let Some(data) = paint_state.weightmaps.get_mut(&coord) else {
            continue;
        };
        let origin = coord.world_pos();
        let chunk_min = Vec2::new(origin.x, origin.z);
        let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);

        let mut touched = false;
        for op in ops_for_chunk.iter() {
            if paint_op_intersects_chunk(op, chunk_min, chunk_max) {
                apply_paint_op_to_weights(op, chunk_min, &mut data.weights, data.resolution);
                touched = true;
            }
        }
        if touched {
            update_weightmap_image(data, &mut images);
            if debug_perf.weightmap_stats {
                log_weightmap_stats(coord, data, "apply_ops_batch");
            }
            chunks_updated += 1;
        }
    }

    if !added_ops.is_empty() {
        info!("Applied {} new terrain paint ops", added_ops.len());
        perf.paint_ops_added += added_ops.len() as u32;
    }
    if chunks_updated > 0 {
        perf.paint_chunks_updated += chunks_updated;
    }
    perf.terrain_paint_ms += start.elapsed().as_secs_f32() * 1000.0;
}
