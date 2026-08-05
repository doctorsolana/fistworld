//! chunks systems.

use super::mesh::build_water_mesh;
use super::*;
use bevy::light::NotShadowCaster;

#[derive(Component)]
pub struct WaterChunk;

#[derive(Resource, Default)]
pub struct LoadedWaterChunks {
    pub entries: HashMap<ChunkCoord, Option<Entity>>,
}

#[derive(Resource)]
pub struct WaterRenderAssets {
    pub material: Handle<ToonWaterMaterial>,
}

pub(super) fn cleanup_water_chunks(
    mut commands: Commands,
    mut loaded_water: ResMut<LoadedWaterChunks>,
    loaded_chunks: Res<LoadedChunks>,
) {
    let mut to_remove = Vec::new();
    for (coord, entity) in loaded_water.entries.iter() {
        if !loaded_chunks.chunks.contains(coord) {
            to_remove.push((*coord, *entity));
        }
    }

    for (coord, entity) in to_remove {
        if let Some(entity) = entity {
            commands.entity(entity).despawn();
        }
        loaded_water.entries.remove(&coord);
    }
}

pub(super) fn spawn_water_chunks(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    loaded_chunks: Res<LoadedChunks>,
    streaming: Res<crate::terrain::TerrainStreamingState>,
    mut loaded_water: ResMut<LoadedWaterChunks>,
    mut meshes: ResMut<Assets<Mesh>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    // `LoadedChunks` is a HashSet. Iterating it directly while allowing only
    // two water meshes per frame made rivers appear as random disconnected
    // puddles across an already-visible RTS view. Follow terrain's nearest-
    // first order so water grows outward as one contiguous detailed region.
    let mut candidates = if streaming.desired_order.is_empty() {
        let mut fallback = loaded_chunks.chunks.iter().copied().collect::<Vec<_>>();
        if let Some(center) = streaming.center {
            fallback.sort_unstable_by_key(|coord| {
                (coord.x - center.x).abs().max((coord.z - center.z).abs())
            });
        }
        fallback
    } else {
        streaming
            .desired_order
            .iter()
            .copied()
            .filter(|coord| loaded_chunks.chunks.contains(coord))
            .collect::<Vec<_>>()
    };

    const MAX_WATER_MESHES_PER_FRAME: usize = 12;
    const MAX_CHUNKS_EXAMINED_PER_FRAME: usize = 48;
    let mut spawned = 0usize;
    let mut examined = 0usize;

    for coord in candidates.drain(..) {
        if spawned >= MAX_WATER_MESHES_PER_FRAME || examined >= MAX_CHUNKS_EXAMINED_PER_FRAME {
            break;
        }
        if loaded_water.entries.contains_key(&coord) {
            continue;
        }
        examined += 1;

        let Some(mesh) = build_water_mesh(&terrain, coord) else {
            loaded_water.entries.insert(coord, None);
            continue;
        };

        let chunk_pos = coord.world_pos();
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(render_assets.material.clone()),
                Transform::from_translation(chunk_pos),
                // Blended materials still enter Bevy's shadow pass unless
                // explicitly excluded. The water mesh overlaps the bank, so
                // casting from it creates a solid black moving shoreline.
                NotShadowCaster,
                WaterChunk,
                // Fades with the terrain chunks; the far mesh bakes water colour into its
                // vertices, so at map scale this surface is redundant detail.
                crate::terrain::map_view::water_visibility_range(),
            ))
            .id();
        commands.entity(world_root).add_child(entity);
        loaded_water.entries.insert(coord, Some(entity));
        spawned += 1;
    }
}
