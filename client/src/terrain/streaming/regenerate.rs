//! regenerate systems.

use super::*;

/// Regenerate terrain mesh for dirty chunks.
pub(crate) fn regenerate_dirty_chunks(
    mut delta_state: ResMut<TerrainDeltaState>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    chunk_query: Query<(Entity, &TerrainChunk, &Mesh3d)>,
    debug_perf: Res<DebugPerfSettings>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut paint_state: ResMut<TerrainPaintState>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = Instant::now();
    if delta_state.dirty_queue.is_empty() {
        return;
    }

    let mut dirty_list: Vec<ChunkCoord> = Vec::with_capacity(DIRTY_REGEN_MAX_PER_FRAME);
    while dirty_list.len() < DIRTY_REGEN_MAX_PER_FRAME {
        let Some(coord) = delta_state.dirty_queue.pop_front() else {
            break;
        };
        if delta_state.dirty_chunks.remove(&coord) {
            dirty_list.push(coord);
        }
    }
    if dirty_list.is_empty() {
        return;
    }

    let dirty: HashSet<ChunkCoord> = dirty_list.iter().copied().collect();
    if debug_perf.render_diag_logging {
        debug!(
            "Regenerating {} dirty terrain chunks this frame ({} pending)",
            dirty.len(),
            delta_state.dirty_queue.len()
        );
    }
    perf.terrain_chunks_regen += dirty.len() as u32;

    // Despawn dirty loaded chunks so they get regenerated.
    // Important: do not eagerly cancel tasks for chunks that are not currently loaded.
    // Canceling those can starve initial terrain appearance when large dirty queues are present.
    for (entity, chunk, mesh_handle) in chunk_query.iter() {
        if dirty.contains(&chunk.coord) {
            tasks.tasks.remove(&chunk.coord);
            meshes.remove(mesh_handle.0.id());
            materials.remove(chunk.material.id());
            images.remove(chunk.weightmap.id());
            paint_state.weightmaps.remove(&chunk.coord);
            commands.entity(entity).despawn();
            loaded_chunks.chunks.remove(&chunk.coord);
        }
    }
    perf.terrain_regen_ms += start.elapsed().as_secs_f32() * 1000.0;
}

pub(crate) fn process_chunk_tasks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    settings: Res<GraphicsSettings>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut paint_state: ResMut<TerrainPaintState>,
    paint_index: Res<TerrainPaintSpatialIndex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    debug_settings: Res<TerrainDebugSettings>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    mut commands: Commands,
    mut perf: ResMut<PerfHitchStats>,
    debug_perf: Res<DebugPerfSettings>,
) {
    let start = Instant::now();
    let Ok(player_pos) = player_query.single() else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };
    let Some(render_assets) = render_assets else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);
    let view_distance = settings.view_distance;

    // Bootstrap mode: prioritize getting at least nearby terrain visible quickly after connect.
    // Once chunks exist, return to stricter per-frame finalize budget.
    let bootstrap = loaded_chunks.chunks.is_empty();
    let finalize_budget_ms = if bootstrap { 30.0f32 } else { 8.0f32 };
    let finalize_max_per_frame = if bootstrap {
        TASK_FINALIZE_MAX_BOOTSTRAP_PER_FRAME
    } else {
        TASK_FINALIZE_MAX_PER_FRAME
    };
    let mut completed: Vec<ChunkBuildResult> = Vec::new();
    let mut to_remove: Vec<ChunkCoord> = Vec::new();

    let mut ordered_coords: Vec<(ChunkCoord, i32)> = Vec::new();
    for coord in tasks.tasks.keys().copied() {
        let dx = (coord.x - player_chunk.x).abs();
        let dz = (coord.z - player_chunk.z).abs();
        if dx > view_distance || dz > view_distance || !coord.in_world_bounds() {
            to_remove.push(coord);
            continue;
        }
        ordered_coords.push((coord, dx.max(dz)));
    }
    ordered_coords.sort_by_key(|(_, dist)| *dist);

    for (coord, _) in ordered_coords {
        if completed.len() >= finalize_max_per_frame {
            break;
        }

        if start.elapsed().as_secs_f32() * 1000.0 > finalize_budget_ms {
            break;
        }

        let Some(task) = tasks.tasks.get_mut(&coord) else {
            continue;
        };

        if let Some(result) = block_on(poll_once(task)) {
            completed.push(result);
            to_remove.push(coord);
        }
    }

    for coord in to_remove {
        tasks.tasks.remove(&coord);
    }

    let mut finalized = 0u32;
    for mut result in completed {
        if loaded_chunks.chunks.contains(&result.coord) {
            continue;
        }

        // Ensure any paint ops added while the task was running are applied.
        if !paint_state.ops.is_empty() {
            let origin = result.coord.world_pos();
            let chunk_min = Vec2::new(origin.x, origin.z);
            let chunk_max = chunk_min + Vec2::splat(CHUNK_SIZE);
            if let Some(op_ids) = paint_index.ops_by_chunk.get(&result.coord) {
                for op_id in op_ids {
                    if result.op_ids.contains(op_id) {
                        continue;
                    }
                    let Some(op) = paint_state.ops.get(op_id) else {
                        continue;
                    };
                    if paint_op_intersects_chunk(op, chunk_min, chunk_max) {
                        apply_paint_op_to_weights(
                            op,
                            chunk_min,
                            &mut result.weights,
                            result.resolution,
                        );
                    }
                }
            }
        }

        let weightmap =
            build_weightmap_from_weights(result.weights, result.resolution, &mut images);
        if debug_perf.weightmap_stats {
            log_weightmap_stats(result.coord, &weightmap, "spawn");
        }

        let tangents_ref = if result.tangents.is_empty() {
            None
        } else {
            Some(&result.tangents)
        };
        let mesh_handle = build_terrain_mesh(&result.mesh_data, tangents_ref, &mut meshes);

        let material = materials.add(TerrainSplatMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.9,
                metallic: 0.0,
                reflectance: 0.2,
                ..default()
            },
            extension: TerrainSplatExtension {
                weight_map: weightmap.handle.clone(),
                albedo_array: render_assets.albedo_array.clone(),
                normal_array: render_assets.normal_array.clone(),
                layer_tiling: render_assets.layer_tiling,
                debug_mode: debug_settings.mode,
                normal_strength: desired_terrain_normal_strength(
                    result.coord,
                    player_chunk,
                    view_distance,
                ),
            },
        });

        let chunk_pos = result.coord.world_pos();
        let chunk_entity = commands
            .spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(chunk_pos),
                TerrainChunk {
                    coord: result.coord,
                    weightmap: weightmap.handle.clone(),
                    material: material.clone(),
                },
                TerrainMaterialLod { use_lite: false },
            ))
            .id();
        commands.entity(world_root).add_child(chunk_entity);

        paint_state.weightmaps.insert(result.coord, weightmap);
        loaded_chunks.chunks.insert(result.coord);
        finalized += 1;
    }

    if finalized > 0 {
        perf.terrain_chunks_finalized += finalized;
    }
    perf.terrain_finalize_ms += start.elapsed().as_secs_f32() * 1000.0;
}
