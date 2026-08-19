//! far terrain systems.

use super::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::render::render_resource::PrimitiveTopology;

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
                // The material starts with a zero-sized hole. Keep the state
                // truthful until the first complete detail square is ready.
                hole_filled: true,
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(render_assets.far_mesh_material.clone()),
            Transform::from_translation(Vec3::new(origin.x, FAR_TERRAIN_Y_OFFSET, origin.y)),
            Visibility::Visible,
            NotShadowCaster,
        ))
        .id();

    commands.entity(world_root).add_child(entity);

    // Infinite-ocean skirt: continue the far ocean horizontally past the map
    // bounds. The old version was one plane 50m below sea level, which exposed
    // a literal cliff at the boundary in the opening boat shot. This frame has
    // a hole exactly the size of the map, meets its far-water surface at the
    // same height, and uses the same material/day-night response.
    let water_level = terrain
        .generator
        .loaded_map()
        .heightmap
        .water_level
        .unwrap_or(0.0);
    let bounds = terrain.generator.active_map_bounds();
    let skirt = commands
        .spawn((
            OceanSkirt,
            Mesh3d(meshes.add(build_ocean_skirt_mesh(bounds, water_level))),
            MeshMaterial3d(render_assets.far_mesh_material.clone()),
            Transform::default(),
            Visibility::Visible,
            NotShadowCaster,
        ))
        .id();
    commands.entity(world_root).add_child(skirt);
}

/// Marker for the endless-sea sheet outside the map bounds.
#[derive(Component)]
pub struct OceanSkirt;

const OCEAN_SKIRT_MARGIN: f32 = 50_000.0;

/// Four quads surrounding (but never covering) the playable rectangle.
///
/// East/west own the corner quadrants while north/south stop at the map's X
/// bounds. Keeping the quads disjoint avoids double-shaded seams at corners.
fn build_ocean_skirt_mesh(bounds: shared::map::MapBounds, water_level: f32) -> Mesh {
    let min_x = bounds.min[0];
    let min_z = bounds.min[1];
    let max_x = bounds.max[0];
    let max_z = bounds.max[1];
    let outer_min_x = min_x - OCEAN_SKIRT_MARGIN;
    let outer_min_z = min_z - OCEAN_SKIRT_MARGIN;
    let outer_max_x = max_x + OCEAN_SKIRT_MARGIN;
    let outer_max_z = max_z + OCEAN_SKIRT_MARGIN;
    // Match the far mesh's stable map-water height exactly.
    let y = water_level + crate::terrain::map_view::FAR_WATER_SURFACE_OFFSET + FAR_TERRAIN_Y_OFFSET;

    let mut positions = Vec::with_capacity(16);
    let mut normals = Vec::with_capacity(16);
    let mut uvs = Vec::with_capacity(16);
    let mut colors = Vec::with_capacity(16);
    let mut indices = Vec::with_capacity(24);

    let mut add_quad = |x0: f32, z0: f32, x1: f32, z1: f32| {
        let base = positions.len() as u32;
        positions.extend_from_slice(&[[x0, y, z0], [x1, y, z0], [x0, y, z1], [x1, y, z1]]);
        normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
        uvs.extend_from_slice(&[
            [x0 / CHUNK_SIZE, z0 / CHUNK_SIZE],
            [x1 / CHUNK_SIZE, z0 / CHUNK_SIZE],
            [x0 / CHUNK_SIZE, z1 / CHUNK_SIZE],
            [x1 / CHUNK_SIZE, z1 / CHUNK_SIZE],
        ]);
        // RGB is the same fully-deep ramp endpoint as the far map mesh;
        // alpha=0 marks these vertices as ocean in far_terrain.wgsl.
        colors.extend_from_slice(&[[0.035, 0.105, 0.25, 0.0]; 4]);
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    };

    add_quad(outer_min_x, outer_min_z, min_x, outer_max_z);
    add_quad(max_x, outer_min_z, outer_max_x, outer_max_z);
    add_quad(min_x, outer_min_z, max_x, min_z);
    add_quad(min_x, max_z, max_x, outer_max_z);

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(positions),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(normals),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::Float32x2(uvs));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

pub(crate) fn update_far_terrain_hole(
    mut far_query: Query<&mut FarTerrainState, With<FarTerrain>>,
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    streaming: Res<TerrainStreamingState>,
    loaded_chunks: Res<LoadedChunks>,
    loaded_water: Option<Res<crate::water::chunks::LoadedWaterChunks>>,
    settings: Res<GraphicsSettings>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    mut materials: ResMut<Assets<crate::terrain::materials::FarTerrainMaterial>>,
) {
    if !settings.far_terrain_enabled {
        return;
    }

    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };
    let Ok(mut state) = far_query.single_mut() else {
        return;
    };
    let Some(render_assets) = render_assets else {
        return;
    };

    let view_distance = if streaming.render_distance > 0 {
        streaming.render_distance
    } else {
        settings.view_distance
    };
    let center_chunk = ChunkCoord::from_world_pos(anchor_pos);
    let center_cell = IVec2::new(center_chunk.x, center_chunk.z);

    // Fill the hole before detail chunks switch away, or their removal would reveal void
    // instead of map underneath. At close zoom, move an existing hole only after
    // the complete new detail square is ready. Re-filling the whole hole while crossing
    // every 64m chunk boundary put the coarse and detailed land surfaces on top of one
    // another; their different tessellation then appeared to flicker as the camera moved.
    // `update_terrain_chunks` temporarily retains only the old hole's protected chunks,
    // so leaving the previous cutout in place cannot expose a streaming gap behind us.
    let zoom = camera_query
        .iter()
        .next()
        .and_then(|(_, controller)| controller.map(|c| c.zoom))
        .unwrap_or(0.0);
    let detail_ready = !streaming.desired_order.is_empty()
        && loaded_water.as_ref().is_some_and(|water| {
            streaming.desired_order.iter().all(|coord| {
                loaded_chunks.chunks.contains(coord) && water.entries.contains_key(coord)
            })
        });
    let hole_filled = zoom > crate::terrain::map_view::HOLE_FILL_ZOOM;

    // Preserve the previous close-view hole while the leading chunk ring is
    // arriving. On initial load (or after returning from map view) the existing
    // state is filled, so the far mesh remains a safe fallback until detail is
    // genuinely complete.
    if !hole_filled && !detail_ready {
        return;
    }

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
        far_terrain_hole_half(view_distance)
    };
    let Some(mut material) = materials.get_mut(&render_assets.far_mesh_material) else {
        return;
    };
    material.extension.water_params.y = inner_center.x;
    material.extension.water_params.z = inner_center.y;
    material.extension.water_params.w = inner_half;

    state.center_cell = center_cell;
    state.view_distance = view_distance;
    state.hole_filled = hole_filled;
}

/// Half extent of the land cutout beneath the detail square.
///
/// The cutout meets the streamed square without overlap. Coarse and detailed
/// terrain are sampled at different resolutions; drawing them on top of each
/// other produces moire-like bands and apparent terrain warping.
fn far_terrain_hole_half(view_distance: i32) -> f32 {
    (view_distance as f32 + 0.5) * CHUNK_SIZE
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutout_meets_detail_square_without_overlap() {
        let view_distance = 8;
        let detail_half = (view_distance as f32 + 0.5) * CHUNK_SIZE;
        assert_eq!(detail_half, far_terrain_hole_half(view_distance));
    }
}
