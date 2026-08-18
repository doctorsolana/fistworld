//! Visual continuation of animated ocean beyond the playable map boundary.
//!
//! Gameplay terrain, collision and water chunks remain strictly in-bounds.
//! This local patch follows the streaming anchor only while a map edge is
//! close enough to see, and uses the exact same material as ordinary ocean.

use super::*;
use bevy::light::NotShadowCaster;

use crate::terrain::map_view::WATER_FADE_END;
use crate::terrain::TerrainStreamingState;

/// Every physical edge of this patch lies past the shader's radial fade, even
/// with the streaming anchor quantized to a chunk centre.
const EDGE_OCEAN_HALF_EXTENT: f32 = 1_152.0;
/// The 13m shortest swell contributes only 13% of displacement; an 8m grid
/// keeps the main 24m/42m swells smooth while staying a single modest draw.
const EDGE_OCEAN_SPACING: f32 = 8.0;

const _: () = assert!(EDGE_OCEAN_HALF_EXTENT > WATER_FADE_END + CHUNK_SIZE * 0.5);

#[derive(Component)]
pub(super) struct OceanEdgeExtension;

pub(super) fn ensure_ocean_edge_extension(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    existing: Query<(), With<OceanEdgeExtension>>,
) {
    if !existing.is_empty() || terrain.water_level().is_none() {
        return;
    }
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let entity = commands
        .spawn((
            OceanEdgeExtension,
            Mesh3d(meshes.add(build_ocean_edge_mesh())),
            MeshMaterial3d(render_assets.material.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            crate::terrain::map_view::water_visibility_range(),
        ))
        .id();
    commands.entity(world_root).add_child(entity);
}

pub(super) fn update_ocean_edge_extension(
    terrain: Res<WorldTerrain>,
    streaming: Res<TerrainStreamingState>,
    mut extension: Query<(&mut Transform, &mut Visibility), With<OceanEdgeExtension>>,
) {
    let Ok((mut transform, mut visibility)) = extension.single_mut() else {
        return;
    };
    let Some(water_level) = terrain.water_level() else {
        *visibility = Visibility::Hidden;
        return;
    };
    let Some(center_chunk) = streaming.center else {
        *visibility = Visibility::Hidden;
        return;
    };

    let chunk_origin = center_chunk.world_pos();
    let focus = Vec2::new(
        chunk_origin.x + CHUNK_SIZE * 0.5,
        chunk_origin.z + CHUNK_SIZE * 0.5,
    );
    let bounds = terrain.generator.active_map_bounds();
    let distance_to_edge = (focus.x - bounds.min[0])
        .abs()
        .min((bounds.max[0] - focus.x).abs())
        .min((focus.y - bounds.min[1]).abs())
        .min((bounds.max[1] - focus.y).abs());

    // At this distance every out-of-bounds fragment is already fully
    // transparent, so hiding the patch cannot introduce a pop.
    *visibility = if distance_to_edge <= WATER_FADE_END {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    transform.translation = Vec3::new(focus.x, water_level, focus.y);
    transform.rotation = Quat::IDENTITY;
    transform.scale = Vec3::ONE;
}

fn build_ocean_edge_mesh() -> Mesh {
    let segments = (EDGE_OCEAN_HALF_EXTENT * 2.0 / EDGE_OCEAN_SPACING) as usize;
    let vertex_count = (segments + 1) * (segments + 1);

    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut colors = Vec::with_capacity(vertex_count);
    for zi in 0..=segments {
        let z = -EDGE_OCEAN_HALF_EXTENT + zi as f32 * EDGE_OCEAN_SPACING;
        for xi in 0..=segments {
            let x = -EDGE_OCEAN_HALF_EXTENT + xi as f32 * EDGE_OCEAN_SPACING;
            positions.push([x, shared::water::WATER_SURFACE_OFFSET, z]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([x / CHUNK_SIZE, z / CHUNK_SIZE]);
            // B > 1 is a private marker telling toon_water.wgsl to discard
            // this patch inside the playable rectangle. The other channels
            // select fully deep, far-from-shore ocean behavior.
            colors.push([1.0, 1.0, 2.0, 1.0]);
        }
    }

    let row = segments + 1;
    let mut indices = Vec::with_capacity(segments * segments * 6);
    for zi in 0..segments {
        for xi in 0..segments {
            let a = (zi * row + xi) as u32;
            let b = a + 1;
            let c = a + row as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_grid_has_expected_compact_size() {
        let mesh = build_ocean_edge_mesh();
        assert_eq!(mesh.count_vertices(), 83_521);
    }
}
