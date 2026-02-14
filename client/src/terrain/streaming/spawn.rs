//! spawn systems.

use super::*;

/// Determine which chunks should be loaded based on player position.
pub(crate) fn update_terrain_chunks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut streaming: ResMut<TerrainStreamingState>,
    chunk_query: Query<(Entity, &TerrainChunk, &Mesh3d)>,
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
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);
    let view_distance = settings.view_distance;

    // Recompute when player moves to new chunk OR when view distance setting changes.
    let should_recompute = streaming.center != Some(player_chunk) || settings.is_changed();
    if !should_recompute {
        return;
    }
    streaming.center = Some(player_chunk);

    // Update desired chunk ordering (nearest -> farthest).
    let mut desired: Vec<ChunkCoord> = player_chunk.chunks_in_radius(view_distance);
    desired.retain(|c| c.in_world_bounds());
    desired.sort_by_key(|c| {
        let dx = (c.x - player_chunk.x).abs();
        let dz = (c.z - player_chunk.z).abs();
        // Chebyshev distance for square radius ordering.
        dx.max(dz)
    });
    streaming.desired_order = desired;

    // Unload chunks that are now out of range.
    let mut to_remove: Vec<(
        Entity,
        ChunkCoord,
        Handle<Mesh>,
        Handle<TerrainSplatMaterial>,
        Handle<Image>,
    )> = Vec::new();
    for (entity, chunk, mesh_handle) in chunk_query.iter() {
        let dx = (chunk.coord.x - player_chunk.x).abs();
        let dz = (chunk.coord.z - player_chunk.z).abs();
        if dx > view_distance || dz > view_distance || !chunk.coord.in_world_bounds() {
            to_remove.push((
                entity,
                chunk.coord,
                mesh_handle.0.clone(),
                chunk.material.clone(),
                chunk.weightmap.clone(),
            ));
        }
    }
    let removed_count = to_remove.len();
    if removed_count > 0 {
        for (_, coord, _, _, _) in to_remove.iter() {
            tasks.tasks.remove(coord);
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
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    settings: Res<GraphicsSettings>,
    streaming: Res<TerrainStreamingState>,
    mut terrain_materials: ResMut<Assets<TerrainSplatMaterial>>,
    chunks: Query<(Entity, &TerrainChunk, &TerrainMaterialLod)>,
) {
    let Ok(player_pos) = player_query.single() else {
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
    let splat_normal_radius = ((render_distance as f32) * SPLAT_NORMAL_RATIO)
        .round()
        .max(1.0) as i32;
    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);

    for (entity, chunk, lod) in chunks.iter() {
        let dx = (chunk.coord.x - player_chunk.x).abs();
        let dz = (chunk.coord.z - player_chunk.z).abs();
        let dist = dx.max(dz);
        let desired_normal_strength = if dist <= splat_normal_radius {
            1.0
        } else {
            0.0
        };
        if let Some(mat) = terrain_materials.get_mut(&chunk.material) {
            if (mat.extension.normal_strength - desired_normal_strength).abs() > 0.01 {
                mat.extension.normal_strength = desired_normal_strength;
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
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    loaded_chunks: Res<LoadedChunks>,
    streaming: Res<TerrainStreamingState>,
    terrain: Res<WorldTerrain>,
    settings: Res<GraphicsSettings>,
    paint_state: Res<TerrainPaintState>,
    paint_index: Res<TerrainPaintSpatialIndex>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = Instant::now();
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let view_distance = settings.view_distance;

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

        if loaded_chunks.chunks.contains(&coord) || tasks.tasks.contains_key(&coord) {
            return true;
        }

        // Snapshot paint ops that intersect this chunk.
        let mut ops_for_chunk: Vec<TerrainPaintOp> = Vec::new();
        if let Some(op_ids) = paint_index.ops_by_chunk.get(&coord) {
            ops_for_chunk.reserve(op_ids.len());
            for op_id in op_ids {
                if let Some(op) = paint_state.ops.get(op_id) {
                    ops_for_chunk.push(op.clone());
                }
            }
        }
        let op_ids: HashSet<u64> = ops_for_chunk.iter().map(|op| op.id).collect();

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
        let seed = WORLD_SEED;
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let generator = TerrainGenerator::new(seed);
            let mesh_data = generator.generate_chunk_with_deltas(&delta_map, coord);
            let tangents = compute_chunk_tangents(&mesh_data).unwrap_or_default();
            let weights = build_weightmap_weights(&generator, coord, &ops_for_chunk, resolution);
            ChunkBuildResult {
                coord,
                mesh_data,
                tangents,
                weights,
                resolution,
                op_ids,
            }
        });

        tasks.tasks.insert(coord, task);
        chunks_spawned += 1;
        true
    };

    // If streaming hasn't initialized yet (no movement / no update), fall back to computing once.
    let use_cached_order = streaming
        .center
        .is_some_and(|center| center == ChunkCoord::from_world_pos(player_pos.0))
        && !streaming.desired_order.is_empty();

    if use_cached_order {
        for coord in streaming.desired_order.iter().copied() {
            if !enqueue_chunk_if_needed(coord) {
                break;
            }
        }
    } else {
        for coord in ChunkCoord::from_world_pos(player_pos.0).chunks_in_radius(view_distance) {
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
