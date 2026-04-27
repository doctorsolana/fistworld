use bevy::prelude::*;
use shared::building::{build_zones_by_chunk, point_in_any_build_zone_entries, BuildZoneEntry};
use shared::building::{BuildingPosition, PlacedBuilding};
use shared::terrain::WorldTerrain;
use std::collections::HashSet;

use crate::render::systems::{ClientWorldRoot, GraphicsSettings};
use crate::terrain::{LoadedChunks, PerfHitchStats};

use super::foliage::needs_foliage_materials;
use super::{
    try_spawn_simple_prop_mesh, BuildZoneChunkIndex, EnvironmentProp, LoadedPropChunks,
    NeedsFoliageMaterials, PendingPropVisibility, PropAssets, PropChunkIndex, PropKindTag,
    SimplePropMeshCache, TreeActiveLod, TreeLodEntities, TreeLodRoot, TreeLodRuntimeState,
};

/// When a new building is placed, invalidate prop chunks that overlap with its build zone.
pub(super) fn invalidate_props_for_new_buildings(
    mut commands: Commands,
    new_buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition), Added<PlacedBuilding>>,
    changed_buildings: Query<Entity, Or<(Changed<PlacedBuilding>, Changed<BuildingPosition>)>>,
    mut removed_buildings: RemovedComponents<PlacedBuilding>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
    mut build_zone_index: ResMut<BuildZoneChunkIndex>,
) {
    let mut added_entities = HashSet::new();
    for (entity, building, position) in new_buildings.iter() {
        added_entities.insert(entity);
        let zone =
            BuildZoneEntry::from_building(position.0, building.building_type, building.rotation);
        let (min_chunk_x, max_chunk_x, min_chunk_z, max_chunk_z) = zone.chunk_bounds();

        // Despawn props in affected chunks and mark for respawn.
        for cx in min_chunk_x..=max_chunk_x {
            for cz in min_chunk_z..=max_chunk_z {
                let coord = shared::terrain::ChunkCoord::new(cx, cz);
                if loaded_prop_chunks.chunks.remove(&coord) {
                    if let Some(entities) = prop_chunk_index.by_chunk.remove(&coord) {
                        for entity in entities {
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
        }
    }

    let mut changed_existing = false;
    for entity in changed_buildings.iter() {
        if !added_entities.contains(&entity) {
            changed_existing = true;
            break;
        }
    }
    let removed_any = removed_buildings.read().next().is_some();

    if !added_entities.is_empty() || changed_existing || removed_any {
        build_zone_index.dirty = true;
    }

    // For moved/rotated/removed buildings, rebuild props in loaded chunks from scratch.
    if changed_existing || removed_any {
        for entities in prop_chunk_index.by_chunk.values() {
            for entity in entities {
                commands.entity(*entity).despawn();
            }
        }
        loaded_prop_chunks.chunks.clear();
        prop_chunk_index.by_chunk.clear();
    }
}

/// Rebuild cached build-zone chunk lookup only when building state changes.
pub(super) fn sync_build_zone_chunk_index(
    mut build_zone_index: ResMut<BuildZoneChunkIndex>,
    buildings_query: Query<(&PlacedBuilding, &BuildingPosition)>,
    changed_buildings: Query<
        (),
        Or<(
            Added<PlacedBuilding>,
            Changed<PlacedBuilding>,
            Changed<BuildingPosition>,
        )>,
    >,
) {
    let changed_any = !changed_buildings.is_empty();
    let needs_initial_build = build_zone_index.by_chunk.is_empty() && !buildings_query.is_empty();
    if !build_zone_index.dirty && !changed_any && !needs_initial_build {
        return;
    }

    let buildings: Vec<(Vec3, shared::building::BuildingType, f32)> = buildings_query
        .iter()
        .map(|(building, position)| (position.0, building.building_type, building.rotation))
        .collect();
    build_zone_index.by_chunk = build_zones_by_chunk(&buildings);
    build_zone_index.dirty = false;
}

/// Spawn props for newly loaded terrain chunks.
pub(super) fn spawn_chunk_props(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Res<WorldTerrain>,
    prop_assets: Option<Res<PropAssets>>,
    mut simple_mesh_cache: ResMut<SimplePropMeshCache>,
    gltfs: Option<Res<Assets<bevy::gltf::Gltf>>>,
    gltf_nodes: Option<Res<Assets<bevy::gltf::GltfNode>>>,
    gltf_meshes: Option<Res<Assets<bevy::gltf::GltfMesh>>>,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
    build_zone_index: Res<BuildZoneChunkIndex>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    settings: Res<GraphicsSettings>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = std::time::Instant::now();
    let Some(assets) = prop_assets else { return };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };
    if !settings.props_enabled {
        return;
    }

    // Find chunks that need props (limit per frame to avoid hitching during streaming).
    let max_prop_chunks_per_frame = 1usize;
    let mut coords_to_spawn: Vec<shared::terrain::ChunkCoord> = Vec::new();

    for coord in loaded_chunks.chunks.iter() {
        if loaded_prop_chunks.chunks.contains(coord) {
            continue;
        }
        coords_to_spawn.push(*coord);
        if coords_to_spawn.len() >= max_prop_chunks_per_frame {
            break;
        }
    }

    if coords_to_spawn.is_empty() {
        return;
    }

    let mut spawned_instances = 0u32;
    for coord in coords_to_spawn.iter() {
        let chunk_zones = build_zone_index.by_chunk.get(coord);

        let spawns = shared::props::generate_chunk_prop_spawns(&terrain.generator, *coord);
        for spawn in spawns {
            // Skip props inside build zones.
            if let Some(chunk_zones) = chunk_zones {
                let point_xz = Vec2::new(spawn.position.x, spawn.position.z);
                if point_in_any_build_zone_entries(point_xz, chunk_zones) {
                    continue;
                }
            }

            // Use terrain height (includes modifications).
            let adjusted_y = terrain.get_height(spawn.position.x, spawn.position.z);
            let adjusted_position = Vec3::new(spawn.position.x, adjusted_y, spawn.position.z);

            let prop = commands
                .spawn((
                    EnvironmentProp { chunk: spawn.chunk },
                    spawn.render_tuning,
                    PendingPropVisibility,
                    Transform::from_translation(adjusted_position)
                        .with_rotation(spawn.rotation)
                        .with_scale(Vec3::splat(spawn.scale)),
                    GlobalTransform::default(),
                    Visibility::Hidden,
                    InheritedVisibility::default(),
                ))
                .id();

            if let Some(kind) = spawn.kind {
                commands.entity(prop).insert(PropKindTag(kind));
                if let Some(tree_meshes) = assets.tree_meshes.get(&kind) {
                    // Explicitly spawn LOD meshes for trees to ensure instancing across identical meshes.
                    commands.entity(prop).insert(TreeLodRoot);
                    let mut tree_lods = TreeLodEntities::default();
                    commands.entity(prop).with_children(|parent| {
                        let lod0 = parent
                            .spawn((
                                Name::new("LOD0"),
                                Mesh3d(tree_meshes.lod0.clone()),
                                MeshMaterial3d(tree_meshes.material.clone()),
                                Transform::IDENTITY,
                                Visibility::Inherited,
                                InheritedVisibility::default(),
                            ))
                            .id();
                        tree_lods.lod0 = Some(lod0);
                        if let Some(lod1_mesh) = tree_meshes.lod1.as_ref() {
                            let lod1 = parent
                                .spawn((
                                    Name::new("LOD1"),
                                    Mesh3d(lod1_mesh.clone()),
                                    MeshMaterial3d(tree_meshes.material.clone()),
                                    Transform::IDENTITY,
                                    Visibility::Inherited,
                                    InheritedVisibility::default(),
                                ))
                                .id();
                            tree_lods.lod1 = Some(lod1);
                        }
                    });
                    commands.entity(prop).insert((
                        tree_lods,
                        TreeLodRuntimeState {
                            active_lod: TreeActiveLod::Hidden,
                            casts_shadows: spawn.render_tuning.casts_shadows,
                        },
                    ));
                } else {
                    let spawned_simple = try_spawn_simple_prop_mesh(
                        &mut commands,
                        prop,
                        kind,
                        &assets,
                        &mut simple_mesh_cache,
                        gltfs.as_deref(),
                        gltf_nodes.as_deref(),
                        gltf_meshes.as_deref(),
                    );
                    if !spawned_simple {
                        let scene = assets
                            .scenes
                            .get(&kind)
                            .cloned()
                            .unwrap_or_else(|| asset_server.load(spawn.scene_path.clone()));
                        commands.entity(prop).insert(SceneRoot(scene));
                    }
                }
                if needs_foliage_materials(kind) {
                    commands.entity(prop).insert(NeedsFoliageMaterials);
                }
            } else {
                let scene = asset_server.load(spawn.scene_path.clone());
                commands.entity(prop).insert(SceneRoot(scene));
            }
            prop_chunk_index
                .by_chunk
                .entry(spawn.chunk)
                .or_default()
                .push(prop);
            commands.entity(world_root).add_child(prop);
            spawned_instances += 1;
        }

        loaded_prop_chunks.chunks.insert(*coord);
    }
    if !coords_to_spawn.is_empty() {
        perf.props_chunks_spawned += coords_to_spawn.len() as u32;
    }
    if spawned_instances > 0 {
        perf.props_instances_spawned += spawned_instances;
    }
    perf.props_spawn_ms += start.elapsed().as_secs_f32() * 1000.0;
}

pub(super) fn sync_props_enabled_state(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    props: Query<Entity, With<EnvironmentProp>>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
) {
    if !settings.is_changed() {
        return;
    }
    if settings.props_enabled {
        return;
    }

    for entity in props.iter() {
        commands.entity(entity).despawn();
    }
    loaded_prop_chunks.chunks.clear();
    prop_chunk_index.by_chunk.clear();
}

/// Clean up props when their chunk is unloaded.
pub(super) fn cleanup_chunk_props(
    mut commands: Commands,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
) {
    // Find chunks that are no longer loaded.
    let chunks_to_remove: Vec<shared::terrain::ChunkCoord> = loaded_prop_chunks
        .chunks
        .difference(&loaded_chunks.chunks)
        .cloned()
        .collect();

    for coord in chunks_to_remove {
        if let Some(entities) = prop_chunk_index.by_chunk.remove(&coord) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        loaded_prop_chunks.chunks.remove(&coord);
    }
}
