//! ingest systems.

use super::*;

fn queue_dirty_chunk(delta_state: &mut TerrainDeltaState, coord: ChunkCoord) {
    if !coord.in_world_bounds() {
        return;
    }
    if delta_state.dirty_chunks.insert(coord) {
        delta_state.dirty_queue.push_back(coord);
    }
}

fn edge_value(data: Option<&TerrainDeltaData>, xi: usize, zi: usize) -> f32 {
    data.map(|d| d.get_vertex(xi, zi)).unwrap_or(0.0)
}

/// Returns a bitmask of chunk edges whose delta values changed since the previous version.
fn changed_edge_mask(previous: Option<&TerrainDeltaData>, next: &TerrainDeltaData) -> u8 {
    let mut mask = 0u8;
    let last = CHUNK_RESOLUTION - 1;

    for i in 0..CHUNK_RESOLUTION {
        if (edge_value(previous, 0, i) - next.get_vertex(0, i)).abs() > EDGE_DELTA_EPSILON {
            mask |= EDGE_WEST;
            break;
        }
    }
    for i in 0..CHUNK_RESOLUTION {
        if (edge_value(previous, last, i) - next.get_vertex(last, i)).abs() > EDGE_DELTA_EPSILON {
            mask |= EDGE_EAST;
            break;
        }
    }
    for i in 0..CHUNK_RESOLUTION {
        if (edge_value(previous, i, 0) - next.get_vertex(i, 0)).abs() > EDGE_DELTA_EPSILON {
            mask |= EDGE_SOUTH;
            break;
        }
    }
    for i in 0..CHUNK_RESOLUTION {
        if (edge_value(previous, i, last) - next.get_vertex(i, last)).abs() > EDGE_DELTA_EPSILON {
            mask |= EDGE_NORTH;
            break;
        }
    }

    mask
}

/// Ingest replicated TerrainDeltaChunk components into local WorldTerrain.
pub(crate) fn ingest_delta_chunks_from_server(
    mut terrain: ResMut<WorldTerrain>,
    mut delta_state: ResMut<TerrainDeltaState>,
    replicated_chunks: Query<&TerrainDeltaChunk, Changed<TerrainDeltaChunk>>,
    debug_perf: Res<DebugPerfSettings>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = Instant::now();
    let mut ingested = 0u32;
    for delta_chunk in replicated_chunks.iter() {
        let coord = delta_chunk.coord;
        let last_version = delta_state.chunk_versions.get(&coord).copied().unwrap_or(0);

        // Only update if version is newer.
        if delta_chunk.version > last_version {
            ingested += 1;
            if debug_perf.render_diag_logging {
                debug!(
                    "Ingesting delta chunk {:?} v{} (had v{})",
                    coord, delta_chunk.version, last_version
                );
            }

            // Update local WorldTerrain.
            let delta_data = delta_chunk.to_delta_data();
            let changed_edges = changed_edge_mask(terrain.get_delta_chunk(coord), &delta_data);
            terrain.set_delta_chunk(coord, delta_data);

            // Track version.
            delta_state
                .chunk_versions
                .insert(coord, delta_chunk.version);

            // Always dirty this chunk. Dirty direct neighbors only when matching edge deltas changed.
            queue_dirty_chunk(&mut delta_state, coord);
            if (changed_edges & EDGE_WEST) != 0 {
                queue_dirty_chunk(&mut delta_state, ChunkCoord::new(coord.x - 1, coord.z));
            }
            if (changed_edges & EDGE_EAST) != 0 {
                queue_dirty_chunk(&mut delta_state, ChunkCoord::new(coord.x + 1, coord.z));
            }
            if (changed_edges & EDGE_SOUTH) != 0 {
                queue_dirty_chunk(&mut delta_state, ChunkCoord::new(coord.x, coord.z - 1));
            }
            if (changed_edges & EDGE_NORTH) != 0 {
                queue_dirty_chunk(&mut delta_state, ChunkCoord::new(coord.x, coord.z + 1));
            }
        }
    }
    if ingested > 0 {
        perf.delta_chunks_ingested += ingested;
    }
    perf.terrain_delta_ms += start.elapsed().as_secs_f32() * 1000.0;
}
