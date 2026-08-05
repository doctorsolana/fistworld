//! regenerate systems.

use super::*;

/// A chunk whose heights changed and whose replacement is still building.
///
/// It keeps rendering the old ground until the new mesh exists. Without this
/// the world blinks a hole every time terrain is edited.
#[derive(Component)]
pub(crate) struct StaleChunk;

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

    // Mark dirty chunks stale; DO NOT tear them down yet.
    //
    // This used to despawn the entity and free its mesh, material and weightmap
    // here, and let the replacement arrive whenever the async rebuild finished.
    // That leaves a hole in the world for every frame in between — which is the
    // flicker you see the instant a building levels its plot, because levelling
    // dirties the chunk under it.
    //
    // The old mesh is still perfectly good ground; it is merely out of date. So
    // it keeps rendering until the new one is ready, and `process_chunk_tasks`
    // despawns it in the same command batch that spawns the replacement — one
    // atomic swap, no hole and no overlapping pair.
    //
    // Tasks are still cancelled here so a rebuild in flight against the OLD
    // heights is discarded rather than finalised over the new ones.
    for (entity, chunk, _mesh_handle) in chunk_query.iter() {
        if dirty.contains(&chunk.coord) {
            tasks.remove(&chunk.coord);
            commands.entity(entity).insert(StaleChunk);
            loaded_chunks.chunks.remove(&chunk.coord);
        }
    }
    perf.terrain_regen_ms += start.elapsed().as_secs_f32() * 1000.0;
}

pub(crate) fn process_chunk_tasks(
    anchor: (AnchorPlayer, AnchorCamera),
    settings: Res<GraphicsSettings>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    mut tasks: ResMut<TerrainChunkTasks>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut paint_state: ResMut<TerrainPaintState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut images: ResMut<Assets<Image>>,
    debug_settings: Res<TerrainDebugSettings>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    mut commands: Commands,
    mut perf: ResMut<PerfHitchStats>,
    debug_perf: Res<DebugPerfSettings>,
    mut scratch: ResMut<TerrainTaskScratch>,
    stale_query: Query<(Entity, &TerrainChunk, &Mesh3d), With<StaleChunk>>,
) {
    let start = Instant::now();
    let (player_query, camera_query) = anchor;
    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };
    let Some(render_assets) = render_assets else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(anchor_pos);
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
    scratch.completed.clear();
    scratch.to_remove.clear();

    let should_rebuild_order = tasks.order_dirty
        || tasks.order_center != Some(player_chunk)
        || tasks.order_view_distance != view_distance;
    if should_rebuild_order {
        {
            let TerrainChunkTasks {
                tasks: task_map,
                ordered_coords,
                ..
            } = &mut *tasks;
            ordered_coords.clear();
            for coord in task_map.keys().copied() {
                let dx = (coord.x - player_chunk.x).abs();
                let dz = (coord.z - player_chunk.z).abs();
                if dx > view_distance || dz > view_distance || !coord.in_world_bounds() {
                    scratch.to_remove.push(coord);
                    continue;
                }
                ordered_coords.push(coord);
            }
            ordered_coords.sort_by_key(|coord| {
                let dx = (coord.x - player_chunk.x).abs();
                let dz = (coord.z - player_chunk.z).abs();
                dx.max(dz)
            });
        }

        for coord in scratch.to_remove.drain(..) {
            tasks.tasks.remove(&coord);
        }
        tasks.order_center = Some(player_chunk);
        tasks.order_view_distance = view_distance;
        tasks.order_dirty = false;
    }

    {
        let TerrainChunkTasks {
            tasks: task_map,
            ordered_coords,
            ..
        } = &mut *tasks;
        for coord in ordered_coords.iter().copied() {
            if scratch.completed.len() >= finalize_max_per_frame {
                break;
            }

            if start.elapsed().as_secs_f32() * 1000.0 > finalize_budget_ms {
                break;
            }

            let Some(task) = task_map.get_mut(&coord) else {
                continue;
            };

            if let Some(result) = block_on(poll_once(task)) {
                scratch.completed.push(result);
                scratch.to_remove.push(coord);
            }
        }
    }

    if !scratch.to_remove.is_empty() {
        for coord in scratch.to_remove.drain(..) {
            tasks.tasks.remove(&coord);
        }
        tasks.order_dirty = true;
    }

    let mut finalized = 0u32;
    for result in scratch.completed.drain(..) {
        if loaded_chunks.chunks.contains(&result.coord) {
            continue;
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
        let mesh_handle = build_terrain_mesh(
            &result.mesh_data,
            tangents_ref,
            &result.generator,
            result.coord,
            &mut meshes,
        );

        let palette = crate::terrain::materials::stylized_palette();
        let material = materials.add(TerrainSplatMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.97,
                metallic: 0.0,
                reflectance: 0.08,
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
                water_params: water_params_for_generator(&result.generator),
                palette,
            },
        });

        // Retire the superseded chunk now, in the same batch as the spawn
        // below, so the swap happens between frames rather than across them.
        for (stale_entity, stale_chunk, stale_mesh) in stale_query.iter() {
            if stale_chunk.coord != result.coord {
                continue;
            }
            meshes.remove(stale_mesh.0.id());
            materials.remove(stale_chunk.material.id());
            images.remove(stale_chunk.weightmap.id());
            paint_state.weightmaps.remove(&stale_chunk.coord);
            commands.entity(stale_entity).despawn();
        }

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
                // Dithers this chunk into the far map mesh as the camera pulls back.
                crate::terrain::map_view::detail_visibility_range(),
            ))
            .id();
        // Ablation hook: terrain-chunk shadow casting is the prime suspect for
        // the mid-zoom shadow-pass cost (~5M verts x cascades).
        if std::env::var("FISTFORCE_TERRAIN_SHADOWS").is_ok_and(|v| v == "0") {
            commands
                .entity(chunk_entity)
                .insert(bevy::light::NotShadowCaster);
        }
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
