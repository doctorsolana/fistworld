//! spawn systems.

use super::*;

/// Determine which chunks should be loaded based on the streaming anchor
/// (local player, or the RTS camera focus in the rail build).
pub(crate) fn update_terrain_chunks(
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut streaming: ResMut<TerrainStreamingState>,
    chunk_query: Query<(Entity, &TerrainChunk, &Mesh3d)>,
    far_terrain: Query<&FarTerrainState, With<FarTerrain>>,
    settings: Res<GraphicsSettings>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut paint_state: ResMut<TerrainPaintState>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = Instant::now();
    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(anchor_pos);
    let view_distance = settings.view_distance;

    // Recompute when player moves to new chunk OR when view distance setting changes.
    let should_recompute = streaming.center != Some(player_chunk) || settings.is_changed();
    if !should_recompute && !streaming.unload_pending {
        return;
    }
    if should_recompute {
        streaming.center = Some(player_chunk);
        let view_priority = streaming_view_priority(&camera_query);

        // Visible/in-front chunks stream before the retained safety ring behind
        // the camera. The renderer independently frustum-culls actual draws.
        let mut desired: Vec<ChunkCoord> = player_chunk.chunks_in_radius(view_distance);
        desired.retain(|c| c.in_world_bounds());
        desired.sort_by_key(|coord| chunk_stream_priority(*coord, anchor_pos, view_priority));
        streaming.desired_order = desired;
        streaming.unload_pending = true;
    }

    // Unload chunks that are now out of range. During a close-view streaming
    // handoff the far mesh keeps its previous cutout until the complete new
    // detail square is ready. Retain only chunks beneath that old cutout; all
    // other stale chunks remain safe to release. This bounds rapid-pan memory
    // while preventing the old implementation's whole-map coarse/detail flicker.
    let protected_hole = if settings.far_terrain_enabled {
        far_terrain.single().ok().filter(|state| !state.hole_filled)
    } else {
        None
    };
    let mut to_remove: Vec<(
        Entity,
        ChunkCoord,
        Handle<Mesh>,
        Handle<TerrainSplatMaterial>,
        Handle<Image>,
    )> = Vec::new();
    let mut retained_stale = false;
    for (entity, chunk, mesh_handle) in chunk_query.iter() {
        let dx = (chunk.coord.x - player_chunk.x).abs();
        let dz = (chunk.coord.z - player_chunk.z).abs();
        let outside_desired =
            dx > view_distance || dz > view_distance || !chunk.coord.in_world_bounds();
        let protected_by_previous_hole =
            protected_hole.is_some_and(|state| chunk_is_inside_active_far_hole(chunk.coord, state));
        if outside_desired && !protected_by_previous_hole {
            to_remove.push((
                entity,
                chunk.coord,
                mesh_handle.0.clone(),
                chunk.material.clone(),
                chunk.weightmap.clone(),
            ));
        } else if outside_desired {
            retained_stale = true;
        }
    }
    streaming.unload_pending = retained_stale;
    let removed_count = to_remove.len();
    if removed_count > 0 {
        for (_, coord, _, _, _) in to_remove.iter() {
            tasks.remove(coord);
        }
    }
    for (entity, coord, mesh_handle, material_handle, weight_handle) in to_remove {
        meshes.remove(mesh_handle.id());
        materials.remove(material_handle.id());
        images.remove(weight_handle.id());
        paint_state.weightmaps.remove(&coord);
        commands.entity(entity).despawn();
        loaded_chunks.chunks.remove(&coord);
    }
    if removed_count > 0 {
        perf.terrain_chunks_unloaded += removed_count as u32;
    }
    perf.terrain_update_ms += start.elapsed().as_secs_f32() * 1000.0;
}

fn chunk_is_inside_active_far_hole(coord: ChunkCoord, state: &FarTerrainState) -> bool {
    if state.hole_filled || state.view_distance < 0 {
        return false;
    }
    let dx = (coord.x - state.center_cell.x).abs();
    let dz = (coord.z - state.center_cell.y).abs();
    dx.max(dz) <= state.view_distance
}

pub(crate) fn update_terrain_render_distance(
    settings: Res<GraphicsSettings>,
    mut streaming: ResMut<TerrainStreamingState>,
) {
    let render_distance = settings.view_distance;
    if streaming.render_distance != render_distance {
        streaming.render_distance = render_distance;
    }
}

pub(crate) fn update_terrain_material_lod(
    mut commands: Commands,
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    settings: Res<GraphicsSettings>,
    mut streaming: ResMut<TerrainStreamingState>,
    mut terrain_materials: ResMut<Assets<TerrainSplatMaterial>>,
    chunks: Query<(Entity, &TerrainChunk, &TerrainMaterialLod)>,
) {
    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };
    let render_distance = if streaming.render_distance > 0 {
        streaming.render_distance
    } else {
        settings.view_distance
    };
    if render_distance <= 0 {
        return;
    }
    let splat_normal_radius = splat_normal_radius(render_distance);
    let player_chunk = ChunkCoord::from_world_pos(anchor_pos);
    if streaming.material_lod_center == Some(player_chunk)
        && streaming.material_lod_radius == splat_normal_radius
        && !settings.is_changed()
    {
        return;
    }
    streaming.material_lod_center = Some(player_chunk);
    streaming.material_lod_radius = splat_normal_radius;

    for (entity, chunk, lod) in chunks.iter() {
        let desired_normal_strength =
            desired_terrain_normal_strength(chunk.coord, player_chunk, render_distance);
        if let Some(mut mat) = terrain_materials.get_mut(&chunk.material) {
            if (mat.extension.params.normal_strength - desired_normal_strength).abs() > 0.01 {
                mat.extension.params.normal_strength = desired_normal_strength;
            }
        }

        // Migration path: if this chunk was switched to flat StandardMaterial by an older build,
        // force it back to splat material and clear lite mode.
        if lod.use_lite {
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>();
            commands
                .entity(entity)
                .insert(MeshMaterial3d(chunk.material.clone()));
            commands
                .entity(entity)
                .insert(TerrainMaterialLod { use_lite: false });
            commands.entity(entity).remove::<NotShadowCaster>();
        }
    }
}

/// Spawn terrain chunks that should be loaded but aren't yet.
pub(crate) fn spawn_terrain_chunks(
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    loaded_chunks: Res<LoadedChunks>,
    streaming: Res<TerrainStreamingState>,
    terrain: Res<WorldTerrain>,
    settings: Res<GraphicsSettings>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = Instant::now();
    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };

    let view_distance = settings.view_distance;
    let view_priority = streaming_view_priority(&camera_query);

    // Load new chunks with a soft time budget to avoid hitching.
    let mut chunks_spawned = 0;
    let spawn_budget_ms = 10.0f32;
    let spawn_start = Instant::now();

    let mut enqueue_chunk_if_needed = |coord: ChunkCoord| -> bool {
        if spawn_start.elapsed().as_secs_f32() * 1000.0 > spawn_budget_ms {
            return false;
        }
        if !coord.in_world_bounds() {
            return true;
        }

        if loaded_chunks.chunks.contains(&coord) || tasks.contains_key(&coord) {
            return true;
        }

        // Snapshot delta chunks in a 3x3 area for cross-chunk normal sampling.
        let mut delta_map: HashMap<ChunkCoord, TerrainDeltaData> = HashMap::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let neighbor = ChunkCoord::new(coord.x + dx, coord.z + dz);
                if let Some(delta) = terrain.get_delta_chunk(neighbor) {
                    delta_map.insert(neighbor, delta.clone());
                }
            }
        }

        let resolution = WEIGHTMAP_RESOLUTION;
        // A joining server can replace the process's initial map. Snapshot
        // the active recipe, not TerrainGenerator::new()'s startup cache.
        let generator = terrain.generator.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let mesh_data = generator.generate_chunk_with_deltas(&delta_map, coord);
            let tangents = compute_chunk_tangents(&mesh_data).unwrap_or_default();
            // Shoreline subdivision, attribute copies and tangent fallback are
            // CPU work too. Finish them on the worker, before publishing the
            // result; the main thread only registers the finished mesh asset.
            let mesh = build_terrain_mesh(
                &mesh_data,
                (!tangents.is_empty()).then_some(&tangents),
                &generator,
                coord,
            );
            // Authored surface paint is baked per-chunk map data (like the
            // height deltas), resolved from this generator's map copy.
            let weights = generator
                .loaded_map()
                .edits
                .resolve_chunk_weights(&generator, coord, resolution);
            ChunkBuildResult {
                coord,
                mesh,
                water_params: water_params_for_generator(&generator),
                weights,
                resolution,
            }
        });

        tasks.insert(coord, task);
        chunks_spawned += 1;
        true
    };

    // If streaming hasn't initialized yet (no movement / no update), fall back to computing once.
    let use_cached_order = streaming
        .center
        .is_some_and(|center| center == ChunkCoord::from_world_pos(anchor_pos))
        && !streaming.desired_order.is_empty();

    if use_cached_order {
        let mut desired = streaming.desired_order.clone();
        desired.sort_by_key(|coord| chunk_stream_priority(*coord, anchor_pos, view_priority));
        for coord in desired {
            if !enqueue_chunk_if_needed(coord) {
                break;
            }
        }
    } else {
        for coord in ChunkCoord::from_world_pos(anchor_pos).chunks_in_radius(view_distance) {
            if !enqueue_chunk_if_needed(coord) {
                break;
            }
        }
    }
    if chunks_spawned > 0 {
        perf.terrain_chunks_spawned += chunks_spawned as u32;
    }
    perf.terrain_spawn_ms += start.elapsed().as_secs_f32() * 1000.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_view_hole_protects_only_its_previous_detail_square() {
        let state = FarTerrainState {
            center_cell: IVec2::new(10, -4),
            view_distance: 2,
            hole_filled: false,
        };

        assert!(chunk_is_inside_active_far_hole(
            ChunkCoord::new(8, -6),
            &state
        ));
        assert!(chunk_is_inside_active_far_hole(
            ChunkCoord::new(12, -2),
            &state
        ));
        assert!(!chunk_is_inside_active_far_hole(
            ChunkCoord::new(13, -4),
            &state
        ));
    }

    #[test]
    fn filled_far_mesh_does_not_retain_stale_chunks() {
        let state = FarTerrainState {
            center_cell: IVec2::ZERO,
            view_distance: 8,
            hole_filled: true,
        };
        assert!(!chunk_is_inside_active_far_hole(
            ChunkCoord::new(0, 0),
            &state
        ));
    }
}
