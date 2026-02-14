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

    let origin = far_terrain_origin();
    let spacing = far_terrain_spacing();
    let mesh = build_far_terrain_mesh(&terrain, origin, spacing, FAR_TERRAIN_RESOLUTION);
    let mesh_handle = meshes.add(mesh);

    let entity = commands
        .spawn((
            FarTerrain,
            FarTerrainState {
                center_cell: IVec2::new(i32::MIN, i32::MIN),
                view_distance: -1,
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

pub(crate) fn update_far_terrain_hole(
    mut meshes: ResMut<Assets<Mesh>>,
    mut far_query: Query<(&Mesh3d, &mut FarTerrainState), With<FarTerrain>>,
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    streaming: Res<TerrainStreamingState>,
    settings: Res<GraphicsSettings>,
) {
    if !settings.far_terrain_enabled {
        return;
    }

    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let view_distance = if streaming.render_distance > 0 {
        streaming.render_distance
    } else {
        settings.view_distance
    };
    let center_chunk = ChunkCoord::from_world_pos(player_pos.0);
    let center_cell = IVec2::new(center_chunk.x, center_chunk.z);
    let center_chunk_origin = center_chunk.world_pos();
    let inner_center = Vec2::new(
        center_chunk_origin.x + CHUNK_SIZE * 0.5,
        center_chunk_origin.z + CHUNK_SIZE * 0.5,
    );
    let inner_half = (view_distance as f32 + 0.5) * CHUNK_SIZE + FAR_TERRAIN_INNER_BUFFER;
    let origin = far_terrain_origin();
    let spacing = far_terrain_spacing();

    for (mesh_handle, mut state) in far_query.iter_mut() {
        if state.center_cell == center_cell && state.view_distance == view_distance {
            continue;
        }
        if let Some(mesh) = meshes.get_mut(&mesh_handle.0) {
            rebuild_far_terrain_indices(
                mesh,
                origin,
                spacing,
                inner_center,
                inner_half,
                FAR_TERRAIN_RESOLUTION,
            );
        }
        state.center_cell = center_cell;
        state.view_distance = view_distance;
    }
}

fn far_terrain_origin() -> Vec2 {
    Vec2::new(-WORLD_RADIUS_METERS, -WORLD_RADIUS_METERS)
}

fn far_terrain_spacing() -> f32 {
    let size = WORLD_RADIUS_METERS * 2.0;
    size / (FAR_TERRAIN_RESOLUTION as f32 - 1.0)
}
