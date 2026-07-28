//! far terrain systems.

use super::*;

pub(crate) fn ensure_far_terrain_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    terrain: Res<WorldTerrain>,
    mut existing_far: Query<&mut Visibility, With<FarTerrain>>,
    settings: Res<GraphicsSettings>,
) {
    let desired_visibility = if settings.far_terrain_enabled {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    let mut has_existing = false;
    for mut visibility in existing_far.iter_mut() {
        has_existing = true;
        if *visibility != desired_visibility {
            *visibility = desired_visibility;
        }
    }

    if has_existing || !settings.far_terrain_enabled {
        return;
    }
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let origin = far_terrain_origin(&terrain);
    let spacing = far_terrain_spacing(&terrain);
    let mesh = build_far_terrain_mesh(&terrain, origin, spacing, FAR_TERRAIN_RESOLUTION);
    let mesh_handle = meshes.add(mesh);

    let entity = commands
        .spawn((
            FarTerrain,
            FarTerrainState {
                center_cell: IVec2::new(i32::MIN, i32::MIN),
                view_distance: -1,
                hole_filled: false,
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(render_assets.far_mesh_material.clone()),
            Transform::from_translation(Vec3::new(origin.x, FAR_TERRAIN_Y_OFFSET, origin.y)),
            Visibility::Visible,
            NotShadowCaster,
        ))
        .id();

    commands.entity(world_root).add_child(entity);
}

/// In-flight async rebuild of the far-terrain hole index buffer.
///
/// The hole is recut every time the streaming anchor crosses a chunk boundary,
/// and building ~131k triangles of indices (a ~3MB Vec) is too expensive to do
/// on the main thread; the actual index generation runs on the compute pool
/// and only the (unavoidable) mesh upload happens here.
#[derive(Resource, Default)]
pub(crate) struct FarTerrainHoleTask {
    pending: Option<PendingHoleRebuild>,
}

pub(crate) struct PendingHoleRebuild {
    center_cell: IVec2,
    view_distance: i32,
    hole_filled: bool,
    task: Task<Vec<u32>>,
}

pub(crate) fn update_far_terrain_hole(
    mut meshes: ResMut<Assets<Mesh>>,
    mut far_query: Query<(&Mesh3d, &mut FarTerrainState), With<FarTerrain>>,
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    terrain: Res<WorldTerrain>,
    streaming: Res<TerrainStreamingState>,
    settings: Res<GraphicsSettings>,
    mut hole_task: ResMut<FarTerrainHoleTask>,
) {
    if !settings.far_terrain_enabled {
        return;
    }

    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };
    let Ok((mesh_handle, mut state)) = far_query.single_mut() else {
        return;
    };

    let view_distance = if streaming.render_distance > 0 {
        streaming.render_distance
    } else {
        settings.view_distance
    };
    let center_chunk = ChunkCoord::from_world_pos(anchor_pos);
    let center_cell = IVec2::new(center_chunk.x, center_chunk.z);

    // Apply a finished rebuild (or drop it if the target moved meanwhile).
    if let Some(pending) = hole_task.pending.as_mut() {
        let Some(indices) = block_on(poll_once(&mut pending.task)) else {
            return;
        };
        let (done_cell, done_view, done_filled) = (
            pending.center_cell,
            pending.view_distance,
            pending.hole_filled,
        );
        hole_task.pending = None;
        if let Some(mut mesh) = meshes.get_mut(&mesh_handle.0) {
            mesh.insert_indices(bevy::mesh::Indices::U32(indices));
        }
        state.center_cell = done_cell;
        state.view_distance = done_view;
        state.hole_filled = done_filled;
        // Fall through: if the anchor moved while the task ran, queue the next
        // rebuild immediately below.
    }

    // Fill the hole before the chunks start their dither-out, or the fade would reveal
    // void instead of map underneath. Part of the change check: zooming in place has to
    // trigger a recut just like panning does.
    let zoom = camera_query
        .iter()
        .next()
        .and_then(|(_, controller)| controller.map(|c| c.zoom))
        .unwrap_or(0.0);
    let hole_filled = zoom > crate::terrain::map_view::HOLE_FILL_ZOOM;

    if state.center_cell == center_cell
        && state.view_distance == view_distance
        && state.hole_filled == hole_filled
    {
        return;
    }

    let center_chunk_origin = center_chunk.world_pos();
    let inner_center = Vec2::new(
        center_chunk_origin.x + CHUNK_SIZE * 0.5,
        center_chunk_origin.z + CHUNK_SIZE * 0.5,
    );
    let inner_half = if hole_filled {
        0.0
    } else {
        (view_distance as f32 + 0.5) * CHUNK_SIZE + FAR_TERRAIN_INNER_BUFFER
    };
    let origin = far_terrain_origin(&terrain);
    let spacing = far_terrain_spacing(&terrain);

    hole_task.pending = Some(PendingHoleRebuild {
        center_cell,
        view_distance,
        hole_filled,
        task: AsyncComputeTaskPool::get().spawn(async move {
            build_far_terrain_indices(
                origin,
                spacing,
                inner_center,
                inner_half,
                FAR_TERRAIN_RESOLUTION,
            )
        }),
    });
}

/// Far-terrain extent follows the *actual* map bounds.
///
/// It used to use `WORLD_RADIUS_METERS`, a legacy 90-chunk constant. On any map larger
/// than that the far mesh stopped short and the rest of the world rendered as empty void —
/// an 8km map only got its middle ~5.7km covered.
fn far_terrain_origin(terrain: &WorldTerrain) -> Vec2 {
    let bounds = terrain.generator.active_map_bounds();
    Vec2::new(bounds.min[0], bounds.min[1])
}

fn far_terrain_spacing(terrain: &WorldTerrain) -> f32 {
    let bounds = terrain.generator.active_map_bounds();
    let size = (bounds.max[0] - bounds.min[0]).max(bounds.max[1] - bounds.min[1]);
    size / (FAR_TERRAIN_RESOLUTION as f32 - 1.0)
}
