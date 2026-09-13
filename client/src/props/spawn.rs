use bevy::prelude::*;
use shared::building::{point_in_any_build_zone_entries, FarmFieldClaimChanges};
use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::VillageRoad;
use shared::props::PropSpawn;
use shared::terrain::{WorldTerrain, CHUNK_SIZE};
use std::collections::HashSet;

use crate::render::systems::{ClientWorldRoot, GraphicsSettings};
use crate::streaming::{
    camera_view_distance, chunk_stream_priority, streaming_anchor, streaming_view_priority,
    AnchorCamera, AnchorPlayer,
};
use crate::terrain::{LoadedChunks, PerfHitchStats};

use super::foliage::needs_foliage_materials;
use super::{
    try_spawn_simple_prop_mesh, BuildZoneChunkIndex, EnvironmentProp, LoadedPropChunks,
    NeedsFoliageMaterials, PendingPropSpawns, PendingPropVisibility, PropAssets, PropChunkIndex,
    PropFootprintSources, PropKindTag, SimplePropMeshCache, TreeActiveLod, TreeLodMeshHandles,
    TreeLodRoot, TreeLodRuntimeState,
};

/// Maximum prop instances realized per frame across all pending chunks.
const MAX_PROP_INSTANCE_SPAWNS_PER_FRAME: usize = 48;

/// Accepted crop edits clear only covered roots and refill released ground.
pub(super) fn invalidate_props_for_farm_fields(
    mut commands: Commands,
    mut changes: FarmFieldClaimChanges,
    mut zones: ResMut<BuildZoneChunkIndex>,
    loaded: Res<LoadedPropChunks>,
    mut pending: ResMut<PendingPropSpawns>,
    mut props: ResMut<PropChunkIndex>,
    transforms: Query<&GlobalTransform>,
) {
    let previous = zones.fields.version();
    let touched = changes.sync(&mut zones.bypass_change_detection().fields);
    if zones.fields.version() == previous {
        return;
    }
    zones.dirty = true;
    for coord in touched {
        if let Some(entities) = props.by_chunk.get_mut(&coord) {
            entities.retain(|entity| {
                let covered = transforms.get(*entity).is_ok_and(|transform| {
                    zones.fields.contains_point(transform.translation().xz())
                });
                if covered {
                    commands.entity(*entity).try_despawn();
                }
                !covered
            });
        }
        if loaded.chunks.contains(&coord) {
            pending.request_refill(coord);
        }
    }
}

/// Reconcile actual footprint changes, never component change ticks alone.
/// Interest removals and upgrades release only their previous plots; surviving
/// scenery remains visible while the normal streamer fills newly free ground.
pub(super) fn invalidate_props_for_new_buildings(
    mut commands: Commands,
    changed_buildings: Query<
        (Entity, &PlacedBuilding, &BuildingPosition),
        Or<(Changed<PlacedBuilding>, Changed<BuildingPosition>)>,
    >,
    mut removed_buildings: RemovedComponents<PlacedBuilding>,
    mut removed_positions: RemovedComponents<BuildingPosition>,
    loaded_prop_chunks: Res<LoadedPropChunks>,
    mut pending_spawns: ResMut<PendingPropSpawns>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
    mut build_zone_index: ResMut<BuildZoneChunkIndex>,
    prop_transforms: Query<&GlobalTransform>,
    changed_squares: Query<
        (Entity, &shared::components::SettlementCivicSquare),
        Changed<shared::components::SettlementCivicSquare>,
    >,
    mut removed_squares: RemovedComponents<shared::components::SettlementCivicSquare>,
    mut sources: ResMut<PropFootprintSources>,
    all_sources: (
        Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
        Query<(Entity, &shared::components::SettlementCivicSquare)>,
    ),
) {
    let (all_buildings, all_squares) = all_sources;
    let PropFootprintSources {
        buildings: previous_buildings,
        squares: previous_squares,
        field_revision,
        initialized,
    } = &mut *sources;
    // A world reset may retain entities whose components did not change. Seed
    // the new cache once from live sources, then resume incremental updates.
    let initial = !*initialized;
    *initialized = true;
    // Adding/removing the first/last accepted field changes a farm's inferred
    // reservation even when the building itself has not been re-replicated.
    let fields_changed = *field_revision != build_zone_index.fields.version();
    *field_revision = build_zone_index.fields.version();
    let mut released_zones = Vec::new();
    let mut added_zones = Vec::new();
    for entity in removed_buildings.read().chain(removed_positions.read()) {
        if let Some(previous) = previous_buildings.remove(&entity) {
            released_zones.extend(previous);
        }
    }
    let changed = changed_buildings.iter().chain(
        all_buildings
            .iter()
            .take(if initial || fields_changed {
                usize::MAX
            } else {
                0
            })
            .filter(|(_, building, _)| {
                initial || building.building_type == shared::building::BuildingType::Farmstead
            }),
    );
    for (entity, building, position) in changed {
        let next = build_zone_index.fields.building_zones(
            position.0,
            building.building_type,
            building.rotation,
        );
        if previous_buildings.get(&entity) == Some(&next) {
            continue;
        }
        if let Some(previous) = previous_buildings.insert(entity, next.clone()) {
            released_zones.extend(previous);
        }
        added_zones.extend(next);
    }
    for entity in removed_squares.read() {
        if let Some(previous) = previous_squares.remove(&entity) {
            released_zones.push(previous);
        }
    }
    for (entity, square) in
        changed_squares.iter().chain(
            all_squares
                .iter()
                .take(if initial { usize::MAX } else { 0 }),
        )
    {
        let next = shared::building::BuildZoneEntry::from_rotated_rect(
            square.center.xz(),
            square.half_extents,
            square.rotation,
        );
        if previous_squares.get(&entity) == Some(&next) {
            continue;
        }
        if let Some(previous) = previous_squares.insert(entity, next) {
            released_zones.push(previous);
        }
        added_zones.push(next);
    }
    if added_zones.is_empty() && released_zones.is_empty() {
        return;
    }
    build_zone_index.dirty = true;

    // Only the new plot may remove an existing tree/bush. Adjacent roots and
    // their loaded meshes retain their identity throughout construction.
    for zone in added_zones {
        let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                let coord = shared::terrain::ChunkCoord::new(x, z);
                if let Some(entities) = prop_chunk_index.by_chunk.get_mut(&coord) {
                    entities.retain(|prop| {
                        let Ok(transform) = prop_transforms.get(*prop) else {
                            return true;
                        };
                        if zone.contains_point(transform.translation().xz()) {
                            commands.entity(*prop).try_despawn();
                            false
                        } else {
                            true
                        }
                    });
                }
                for (queued_coord, spawns) in &mut pending_spawns.queue {
                    if *queued_coord == coord {
                        spawns.retain(|spawn| !zone.contains_point(spawn.position.xz()));
                    }
                }
            }
        }
    }
    let mut released_chunks = HashSet::new();
    for zone in released_zones {
        let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                released_chunks.insert(shared::terrain::ChunkCoord::new(x, z));
            }
        }
    }
    for coord in released_chunks {
        // Re-evaluate candidates but keep all surviving roots. Stage 1 filters
        // already-present positions, so a refill neither blinks nor duplicates.
        if loaded_prop_chunks.chunks.contains(&coord) {
            pending_spawns.request_refill(coord);
        }
    }
}

/// Rebuild cached build-zone chunk lookup only when building state changes.
pub(super) fn sync_build_zone_chunk_index(
    mut build_zone_index: ResMut<BuildZoneChunkIndex>,
    buildings_query: Query<(&PlacedBuilding, &BuildingPosition)>,
    squares: Query<&shared::components::SettlementCivicSquare>,
) {
    if !build_zone_index.dirty {
        return;
    }

    let buildings: Vec<(Vec3, shared::building::BuildingType, f32)> = buildings_query
        .iter()
        .map(|(building, position)| (position.0, building.building_type, building.rotation))
        .collect();
    build_zone_index.by_chunk = build_zone_index.fields.building_zones_by_chunk(&buildings);
    for square in &squares {
        let zone = shared::building::BuildZoneEntry::from_rotated_rect(
            square.center.xz(),
            square.half_extents,
            square.rotation,
        );
        let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                build_zone_index
                    .by_chunk
                    .entry(shared::terrain::ChunkCoord::new(x, z))
                    .or_default()
                    .push(zone);
            }
        }
    }
    build_zone_index.dirty = false;
}

/// Spawn props for newly loaded terrain chunks.
///
/// Two-stage streaming: at most one chunk per frame has its spawn list
/// *generated* and enqueued, and at most `MAX_PROP_INSTANCE_SPAWNS_PER_FRAME`
/// instances are *realized* per frame across the queue, so dense forest chunks
/// no longer land as a single-frame hitch.
pub(super) fn spawn_chunk_props(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Res<WorldTerrain>,
    anchor: (AnchorPlayer, AnchorCamera),
    prop_assets: Option<Res<PropAssets>>,
    mut simple_mesh_cache: ResMut<SimplePropMeshCache>,
    gltf_assets: (
        Option<Res<Assets<bevy::gltf::Gltf>>>,
        Option<Res<Assets<bevy::gltf::GltfNode>>>,
        Option<Res<Assets<bevy::gltf::GltfMesh>>>,
    ),
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut pending_spawns: ResMut<PendingPropSpawns>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
    build_zone_index: Res<BuildZoneChunkIndex>,
    scenery: (
        Query<&GlobalTransform>,
        Query<&VillageRoad>,
        Query<Entity, With<ClientWorldRoot>>,
    ),
    settings: Res<GraphicsSettings>,
    mut perf: ResMut<PerfHitchStats>,
) {
    let start = std::time::Instant::now();
    let (player_query, camera_query) = anchor;
    let (gltfs, gltf_nodes, gltf_meshes) = gltf_assets;
    let (existing_transforms, roads, world_root_query) = scenery;
    let Some(assets) = prop_assets else { return };
    let Some(anchor_pos) = streaming_anchor(&player_query, &camera_query) else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };
    if !settings.props_enabled {
        return;
    }

    let camera_distance = camera_view_distance(&camera_query);
    let view_priority = streaming_view_priority(&camera_query);
    if super::props_suppressed_at_zoom(camera_distance) {
        // Cleanup removes already-realized chunks. Drop queued instances here
        // as well so zooming to map view cannot continue realizing scenery for
        // a frame while that cleanup is deferred.
        pending_spawns.queue.clear();
        pending_spawns.refill.clear();
        return;
    }

    // Stage 1: enqueue at most one new chunk's spawn list per frame.
    // Props stream independently from terrain. Terrain can stay loaded farther out for silhouettes,
    // while dense forests/ground clutter should only exist as live entities near the player.
    let player_chunk = shared::terrain::ChunkCoord::from_world_pos(anchor_pos);
    let prop_radius = prop_stream_radius_chunks(&settings, camera_distance);
    // Only one chunk can be generated below. Select that minimum directly
    // instead of allocating and sorting the entire loaded radius every frame.
    let next = (-prop_radius..=prop_radius)
        .flat_map(|dx| {
            (-prop_radius..=prop_radius).map(move |dz| {
                shared::terrain::ChunkCoord::new(player_chunk.x + dx, player_chunk.z + dz)
            })
        })
        .filter(|coord| {
            loaded_chunks.chunks.contains(coord)
                && (!loaded_prop_chunks.chunks.contains(coord)
                    || pending_spawns.refill.contains(coord))
        })
        .min_by_key(|coord| chunk_stream_priority(*coord, anchor_pos, view_priority));

    if let Some(coord) = next {
        let chunk_zones = build_zone_index.by_chunk.get(&coord);
        let mut spawns = shared::props::generate_chunk_prop_spawns(&terrain.generator, coord);
        // Ground detail IS spawned now. It was blanket-dropped here because the
        // only ground cover in the world was a 738-triangle textured tuft, and
        // 38,578 of those were pure cost from a camera 200 m up.
        //
        // Both halves of that changed. The patches are 36 triangles at LOD0 and
        // 12 at LOD1, and they carry a 240 m `visible_end_distance`
        // (props::tuning) so nothing outside that ring is ever submitted. What
        // is left is the carpet you see when you zoom in, which was missing.
        //
        // They are NOT untextured: each carries a 128x128 blade atlas with
        // `alphaMode: MASK` (cutoff 0.5). MASK and not BLEND is the load-bearing
        // part -- cutout keeps depth writes and needs no back-to-front sort,
        // which is what makes thousands of overlapping patches affordable.
        if let Some(chunk_zones) = chunk_zones {
            spawns.retain(|spawn| {
                let point_xz = Vec2::new(spawn.position.x, spawn.position.z);
                !point_in_any_build_zone_entries(point_xz, chunk_zones)
            });
        }
        spawns.retain(|spawn| !build_zone_index.fields.contains_point(spawn.position.xz()));
        spawns.retain(|spawn| {
            // Brush yields to a finished ribbon. Trees do too, but only after
            // the embodied road worker has chopped them and advanced this
            // built prefix; rocks remain permanent scenery and collision.
            let permanent = spawn
                .kind
                .is_some_and(|kind| kind.blocks_village_road() && !kind.is_road_clearable());
            let padding = if spawn.kind.is_some_and(|kind| kind.is_road_clearable()) {
                shared::components::ROAD_CLEARED_TREE_PADDING
            } else {
                0.12
            };
            permanent
                || !roads.iter().any(|road| {
                    road.contains_built_point(
                        Vec2::new(spawn.position.x, spawn.position.z),
                        padding,
                    )
                })
        });
        if let Some(existing) = prop_chunk_index.by_chunk.get(&coord) {
            let occupied = existing
                .iter()
                .filter_map(|entity| existing_transforms.get(*entity).ok())
                .map(|transform| prop_position_key(transform.translation()))
                .collect::<HashSet<_>>();
            spawns.retain(|spawn| !occupied.contains(&prop_position_key(spawn.position)));
        }
        // Mark loaded immediately so the chunk is not re-enqueued while its
        // instances trickle in from the queue.
        loaded_prop_chunks.chunks.insert(coord);
        pending_spawns.refill.remove(&coord);
        perf.props_chunks_spawned += 1;
        if !spawns.is_empty() {
            pending_spawns.queue.push_back((coord, spawns));
        }
    }

    // Stage 2: realize a bounded number of queued instances.
    let mut budget = MAX_PROP_INSTANCE_SPAWNS_PER_FRAME;
    let mut spawned_instances = 0u32;
    while budget > 0 {
        let Some((coord, spawns)) = pending_spawns.queue.front_mut() else {
            break;
        };
        let coord = *coord;
        let take = budget.min(spawns.len());
        for spawn in spawns.drain(..take) {
            let prop = spawn_prop_instance(
                &mut commands,
                &asset_server,
                &terrain,
                &assets,
                &mut simple_mesh_cache,
                gltfs.as_deref(),
                gltf_nodes.as_deref(),
                gltf_meshes.as_deref(),
                world_root,
                spawn,
            );
            prop_chunk_index
                .by_chunk
                .entry(coord)
                .or_default()
                .push(prop);
            spawned_instances += 1;
        }
        budget -= take;
        if pending_spawns
            .queue
            .front()
            .is_some_and(|(_, spawns)| spawns.is_empty())
        {
            pending_spawns.queue.pop_front();
        }
    }

    if spawned_instances > 0 {
        perf.props_instances_spawned += spawned_instances;
    }
    perf.props_spawn_ms += start.elapsed().as_secs_f32() * 1000.0;
}

// Height may change with earthworks; generated horizontal positions remain
// stable. Compare the original coordinates without snapping distinct plants.
fn prop_position_key(position: Vec3) -> (u32, u32) {
    (position.x.to_bits(), position.z.to_bits())
}

/// Remove vegetation covered by newly completed road sections.
///
/// Trees can appear here only after the road worker completed their chopping
/// phase. Rocks remain because every server survey treats them as permanent.
/// Chunk indexes keep this bounded to the few chunks touched by the path.
pub(super) fn clear_props_for_built_village_roads(
    mut commands: Commands,
    changed_roads: Query<&VillageRoad, Changed<VillageRoad>>,
    mut pending: ResMut<PendingPropSpawns>,
    mut index: ResMut<PropChunkIndex>,
    props: Query<(&GlobalTransform, Option<&PropKindTag>)>,
) {
    for road in changed_roads.iter() {
        let Some((min_chunk, max_chunk)) =
            road_chunk_bounds(road, shared::components::ROAD_CLEARED_TREE_PADDING)
        else {
            continue;
        };
        for cx in min_chunk.x..=max_chunk.x {
            for cz in min_chunk.z..=max_chunk.z {
                let coord = shared::terrain::ChunkCoord::new(cx, cz);
                if let Some(entities) = index.by_chunk.get_mut(&coord) {
                    entities.retain(|entity| {
                        let Ok((transform, kind)) = props.get(*entity) else {
                            return true;
                        };
                        if kind.is_some_and(|kind| {
                            kind.0.blocks_village_road() && !kind.0.is_road_clearable()
                        }) {
                            return true;
                        }
                        let at = transform.translation();
                        let padding = if kind.is_some_and(|kind| kind.0.is_road_clearable()) {
                            shared::components::ROAD_CLEARED_TREE_PADDING
                        } else {
                            0.12
                        };
                        if road.contains_built_point(Vec2::new(at.x, at.z), padding) {
                            commands.entity(*entity).despawn();
                            false
                        } else {
                            true
                        }
                    });
                }
                for (queued, spawns) in pending.queue.iter_mut() {
                    if *queued != coord {
                        continue;
                    }
                    spawns.retain(|spawn| {
                        let padding = if spawn.kind.is_some_and(|kind| kind.is_road_clearable()) {
                            shared::components::ROAD_CLEARED_TREE_PADDING
                        } else {
                            0.12
                        };
                        spawn.kind.is_some_and(|kind| {
                            kind.blocks_village_road() && !kind.is_road_clearable()
                        }) || !road.contains_built_point(
                            Vec2::new(spawn.position.x, spawn.position.z),
                            padding,
                        )
                    });
                }
            }
        }
    }
}

fn road_chunk_bounds(
    road: &VillageRoad,
    padding: f32,
) -> Option<(shared::terrain::ChunkCoord, shared::terrain::ChunkCoord)> {
    let mut points = road.built_points().iter().copied();
    let first = points.next()?;
    let (mut min, mut max) = (first, first);
    for point in points {
        min = min.min(point);
        max = max.max(point);
    }
    let extent = road.width * 0.5 + padding;
    min -= Vec2::splat(extent);
    max += Vec2::splat(extent);
    Some((
        shared::terrain::ChunkCoord::new(
            (min.x / CHUNK_SIZE).floor() as i32,
            (min.y / CHUNK_SIZE).floor() as i32,
        ),
        shared::terrain::ChunkCoord::new(
            (max.x / CHUNK_SIZE).floor() as i32,
            (max.y / CHUNK_SIZE).floor() as i32,
        ),
    ))
}

pub(super) fn spawn_prop_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    terrain: &WorldTerrain,
    assets: &PropAssets,
    simple_mesh_cache: &mut SimplePropMeshCache,
    gltfs: Option<&Assets<bevy::gltf::Gltf>>,
    gltf_nodes: Option<&Assets<bevy::gltf::GltfNode>>,
    gltf_meshes: Option<&Assets<bevy::gltf::GltfMesh>>,
    world_root: Entity,
    spawn: PropSpawn,
) -> Entity {
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
            // Keep trees as single render entities. The LOD system swaps mesh handles on
            // the root instead of maintaining child hierarchies for every tree instance.
            commands.entity(prop).insert((
                Mesh3d(tree_meshes.lod0.clone()),
                MeshMaterial3d(tree_meshes.material.clone()),
                TreeLodRoot,
                TreeLodMeshHandles {
                    lod0: tree_meshes.lod0.clone(),
                    lod1: tree_meshes.lod1.clone(),
                },
                TreeLodRuntimeState {
                    active_lod: TreeActiveLod::Hidden,
                    casts_shadows: spawn.render_tuning.casts_shadows,
                },
            ));
        } else {
            let spawned_simple = try_spawn_simple_prop_mesh(
                commands,
                asset_server,
                prop,
                kind,
                assets,
                simple_mesh_cache,
                gltfs,
                gltf_nodes,
                gltf_meshes,
            );
            if !spawned_simple {
                let scene = assets
                    .scenes
                    .get(&kind)
                    .cloned()
                    .unwrap_or_else(|| asset_server.load(spawn.scene_path.clone()));
                commands.entity(prop).insert(WorldAssetRoot(scene));
            }
        }
        if needs_foliage_materials(kind) {
            commands.entity(prop).insert(NeedsFoliageMaterials);
        }
    } else {
        let scene = asset_server.load(spawn.scene_path.clone());
        commands.entity(prop).insert(WorldAssetRoot(scene));
    }

    commands.entity(world_root).add_child(prop);
    prop
}

pub(super) fn sync_props_enabled_state(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    props: Query<Entity, With<EnvironmentProp>>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut pending_spawns: ResMut<PendingPropSpawns>,
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
    pending_spawns.queue.clear();
    pending_spawns.refill.clear();
    prop_chunk_index.by_chunk.clear();
}

/// Clean up props when their chunk is unloaded.
pub(super) fn cleanup_chunk_props(
    mut commands: Commands,
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    settings: Res<GraphicsSettings>,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    mut pending_spawns: ResMut<PendingPropSpawns>,
    mut prop_chunk_index: ResMut<PropChunkIndex>,
) {
    let camera_distance = camera_view_distance(&camera_query);
    let suppress_props = super::props_suppressed_at_zoom(camera_distance);
    let player_chunk = streaming_anchor(&player_query, &camera_query)
        .map(shared::terrain::ChunkCoord::from_world_pos);
    let prop_radius = prop_stream_radius_chunks(&settings, camera_distance);

    // Full map view needs no individual scenery at all. Otherwise find chunks
    // that are no longer loaded or are outside the tighter prop radius.
    let chunks_to_remove: Vec<shared::terrain::ChunkCoord> = loaded_prop_chunks
        .chunks
        .iter()
        .copied()
        .filter(|coord| {
            if suppress_props {
                return true;
            }
            if !loaded_chunks.chunks.contains(coord) {
                return true;
            }
            let Some(player_chunk) = player_chunk else {
                return false;
            };
            !chunk_in_prop_radius(*coord, player_chunk, prop_radius)
        })
        .collect();

    for coord in chunks_to_remove {
        if let Some(entities) = prop_chunk_index.by_chunk.remove(&coord) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        pending_spawns.discard_chunk(coord);
        loaded_prop_chunks.chunks.remove(&coord);
    }
}

/// How far props stream, in chunks.
///
/// A fixed radius is wrong for a top-down camera: the visible ground footprint grows
/// with zoom, so a radius tuned for an eye-level view leaves the screen empty as soon as
/// you zoom out. Scale with camera distance instead, and keep the FPS-era override.
fn prop_stream_radius_chunks(settings: &GraphicsSettings, camera_distance: f32) -> i32 {
    let configured = std::env::var("FISTFORCE_PROP_CHUNK_RADIUS")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value >= 0);

    // Ground covered by the view is roughly the camera distance again; pad it so props
    // exist slightly beyond the frame rather than popping in at the edge.
    let visible_ground_radius = (camera_distance * 1.35).max(180.0);
    let default_radius = ((visible_ground_radius * settings.prop_render_multiplier.max(0.25))
        / CHUNK_SIZE)
        .ceil() as i32;

    configured
        .unwrap_or(default_radius)
        .clamp(1, settings.view_distance.max(1))
}

fn chunk_in_prop_radius(
    coord: shared::terrain::ChunkCoord,
    player_chunk: shared::terrain::ChunkCoord,
    prop_radius: i32,
) -> bool {
    let dx = (coord.x - player_chunk.x).abs();
    let dz = (coord.z - player_chunk.z).abs();
    dx <= prop_radius && dz <= prop_radius
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::SettlementCivicSquare;
    use shared::terrain::ChunkCoord;

    fn square_app() -> App {
        let mut app = App::new();
        app.init_resource::<LoadedPropChunks>()
            .init_resource::<PendingPropSpawns>()
            .init_resource::<PropChunkIndex>()
            .init_resource::<BuildZoneChunkIndex>()
            .init_resource::<PropFootprintSources>()
            .add_systems(
                Update,
                (
                    invalidate_props_for_farm_fields,
                    invalidate_props_for_new_buildings,
                    sync_build_zone_chunk_index,
                )
                    .chain(),
            );
        app
    }

    #[test]
    fn world_reset_discards_expired_source_history_and_reseeds_unchanged_entities() {
        #[derive(Resource)]
        struct Playing(bool);

        let mut app = App::new();
        app.insert_resource(Playing(true))
            .init_resource::<LoadedPropChunks>()
            .init_resource::<PendingPropSpawns>()
            .init_resource::<PropChunkIndex>()
            .init_resource::<BuildZoneChunkIndex>()
            .init_resource::<PropFootprintSources>()
            .init_resource::<super::super::ground_cover_chunked::ChunkedGroundCoverState>()
            .init_resource::<Assets<super::super::ground_cover_instancing::InstancedGrassMaterial>>(
            )
            .add_systems(
                Update,
                (
                    invalidate_props_for_farm_fields,
                    invalidate_props_for_new_buildings,
                    sync_build_zone_chunk_index,
                )
                    .chain()
                    .run_if(|playing: Res<Playing>| playing.0),
            );
        let building = || PlacedBuilding {
            building_type: shared::building::BuildingType::LogCabin,
            rotation: 0.0,
        };
        let old = app
            .world_mut()
            .spawn((building(), BuildingPosition(Vec3::new(32., 0., 32.))))
            .id();
        let retained = app
            .world_mut()
            .spawn((building(), BuildingPosition(Vec3::new(160., 0., 32.))))
            .id();
        let old_square = app.world_mut().spawn(square_at(96.)).id();
        app.update();
        assert_eq!(
            app.world()
                .resource::<PropFootprintSources>()
                .buildings
                .len(),
            2
        );

        // Match a disconnect followed by time in the menu: readers do not run
        // while removal messages expire, but their previous source data remains.
        app.world_mut().resource_mut::<Playing>().0 = false;
        app.world_mut().despawn(old);
        app.world_mut().despawn(old_square);
        for _ in 0..3 {
            app.update();
        }
        assert!(app
            .world()
            .resource::<PropFootprintSources>()
            .buildings
            .contains_key(&old));
        super::super::reset_world_streaming(app.world_mut());
        let cache = app.world().resource::<PropFootprintSources>();
        assert!(cache.buildings.is_empty() && cache.squares.is_empty());
        assert_eq!(cache.field_revision, 0);
        assert!(!cache.initialized);

        let new = app
            .world_mut()
            .spawn((building(), BuildingPosition(Vec3::new(288., 0., 32.))))
            .id();
        let new_square = app.world_mut().spawn(square_at(352.)).id();
        app.world_mut().resource_mut::<Playing>().0 = true;
        app.update();
        let cache = app.world().resource::<PropFootprintSources>();
        assert_eq!(
            cache.buildings.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([retained, new])
        );
        assert_eq!(
            cache.squares.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([new_square])
        );

        // The unmodified retained entity was reseeded, so a later removal also
        // releases its old footprint correctly rather than leaving stale land.
        app.world_mut().despawn(retained);
        app.update();
        assert!(!app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .by_chunk
            .contains_key(&ChunkCoord::new(2, 0)));
        assert_eq!(
            app.world()
                .resource::<PropFootprintSources>()
                .buildings
                .len(),
            1
        );
    }

    #[test]
    fn accepted_field_updates_release_only_touched_prop_chunks() {
        use shared::components::{FarmField, FarmFieldShape, PlayerPosition, PlayerRotation};
        let mut app = square_app();
        let at = Vec3::new(32.0, 0.0, 32.0);
        let local = resident_prop(&mut app, at);
        let remote = resident_prop(&mut app, Vec3::new(720.0, 0.0, 720.0));
        let entity = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Test".into(),
                    farmstead: at,
                    plot_index: 0,
                    quality: 1.0,
                    layout_version: 0,
                    shape: None,
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(local).is_err());
        assert!(app.world().get_entity(remote).is_ok());
        assert!(app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .fields
            .contains_point(at.xz()));

        let untouched = resident_prop(&mut app, at + Vec3::X * 12.0);
        let version = app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .fields
            .version();
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<FarmField>()
            .unwrap()
            .quality = 0.2;
        app.update();
        assert!(app.world().get_entity(untouched).is_ok());
        assert_eq!(
            app.world()
                .resource::<BuildZoneChunkIndex>()
                .fields
                .version(),
            version
        );

        app.world_mut()
            .entity_mut(entity)
            .get_mut::<FarmField>()
            .unwrap()
            .shape = Some(FarmFieldShape::default());
        app.update();
        assert!(!app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .fields
            .contains_point(at.xz()));
        assert!(app.world().get_entity(remote).is_ok());
        app.world_mut().despawn(entity);
        app.update();
        assert!(!app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .fields
            .has_farm(at));
        assert!(app.world().get_entity(remote).is_ok());
    }

    #[test]
    fn removing_last_accepted_field_restores_only_the_legacy_farm_reservation() {
        use shared::building::{clearance_zones_for_building, BuildingType};
        use shared::components::{FarmField, FarmFieldShape, PlayerPosition, PlayerRotation};
        let mut app = square_app();
        let at = Vec3::new(32.0, 0.0, 32.0);
        app.world_mut().spawn((
            PlacedBuilding {
                building_type: BuildingType::Farmstead,
                rotation: 0.0,
            },
            BuildingPosition(at),
        ));
        // An explicit empty field is still a worker-authored reservation record:
        // it suppresses the historical rectangular field fallback.
        let field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Test".into(),
                    farmstead: at,
                    plot_index: 0,
                    quality: 1.0,
                    layout_version: 1,
                    shape: Some(FarmFieldShape::default()),
                },
                PlayerPosition(at),
                PlayerRotation(0.0),
            ))
            .id();
        app.update();
        let zones = clearance_zones_for_building(at, BuildingType::Farmstead, 0.0);
        let point = (-90..90)
            .flat_map(|x| (-90..90).map(move |z| at + Vec3::new(x as f32, 0.0, z as f32)))
            .find(|p| {
                zones.iter().skip(1).any(|zone| zone.contains_point(p.xz()))
                    && !zones[0].contains_point(p.xz())
            })
            .expect("farm has legacy crop land outside its building");
        let crop_land = resident_prop(&mut app, point);
        let remote = resident_prop(&mut app, Vec3::new(720.0, 0.0, 720.0));
        app.update();
        assert!(app.world().get_entity(crop_land).is_ok());
        app.world_mut().despawn(field);
        app.update();
        assert!(
            app.world().get_entity(crop_land).is_err(),
            "legacy crop reservation becomes active again"
        );
        assert!(app.world().get_entity(remote).is_ok());
    }

    fn square_at(x: f32) -> SettlementCivicSquare {
        SettlementCivicSquare {
            center: Vec3::new(x, 0.0, 32.0),
            half_extents: Vec2::splat(8.0),
            rotation: 0.0,
            market_position: Vec3::new(x, 0.0, 34.0),
            market_rotation: 0.0,
        }
    }

    #[test]
    fn building_replication_and_local_changes_preserve_unaffected_scenery() {
        use shared::building::BuildingType;
        let mut app = square_app();
        let old = Vec3::new(32.0, 0.0, 32.0);
        let neighbor = resident_prop(&mut app, Vec3::new(2.0, 0.0, 32.0));
        let remote = resident_prop(&mut app, Vec3::new(720.0, 0.0, 720.0));
        let covered = resident_prop(&mut app, old);
        let building = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: 0.0,
                },
                BuildingPosition(old),
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(covered).is_err());
        assert!(app.world().get_entity(neighbor).is_ok());

        // Re-replication and foundation-height changes do not alter an XZ
        // reservation and must not reset the scenery or its queued instances.
        let original_queue = app.world().resource::<PendingPropSpawns>().queue.len();
        app.world_mut().entity_mut(building).insert((
            PlacedBuilding {
                building_type: BuildingType::LogCabin,
                rotation: 0.0,
            },
            BuildingPosition(old + Vec3::Y),
        ));
        app.update();
        assert!(app.world().get_entity(neighbor).is_ok());
        assert!(app.world().get_entity(remote).is_ok());
        assert_eq!(
            app.world().resource::<PendingPropSpawns>().queue.len(),
            original_queue
        );
        assert!(app
            .world()
            .resource::<LoadedPropChunks>()
            .chunks
            .contains(&ChunkCoord::new(0, 0)));

        let newly_covered = resident_prop(&mut app, old + Vec3::X * 64.0);
        app.world_mut()
            .entity_mut(building)
            .insert(BuildingPosition(old + Vec3::X * 64.0));
        app.update();
        assert!(app.world().get_entity(newly_covered).is_err());
        assert!(app.world().get_entity(neighbor).is_ok());
        assert!(app.world().get_entity(remote).is_ok());
        app.world_mut().despawn(building);
        app.update();
        assert!(app.world().get_entity(neighbor).is_ok());
        assert!(app.world().get_entity(remote).is_ok());
        assert!(app
            .world()
            .resource::<LoadedPropChunks>()
            .chunks
            .contains(&ChunkCoord::new(11, 11)));
    }

    fn resident_prop(app: &mut App, point: Vec3) -> Entity {
        let coord = ChunkCoord::from_world_pos(point);
        let entity = app
            .world_mut()
            .spawn(GlobalTransform::from_translation(point))
            .id();
        app.world_mut()
            .resource_mut::<PropChunkIndex>()
            .by_chunk
            .entry(coord)
            .or_default()
            .push(entity);
        app.world_mut()
            .resource_mut::<LoadedPropChunks>()
            .chunks
            .insert(coord);
        // A partially realized chunk also has queued props. Both sides of the
        // stream must reset together or regeneration either stalls or duplicates.
        app.world_mut()
            .resource_mut::<PendingPropSpawns>()
            .queue
            .push_back((coord, Vec::new()));
        entity
    }

    #[test]
    fn civic_square_removal_releases_only_its_old_prop_chunks() {
        let mut app = square_app();
        let old_chunk = ChunkCoord::new(0, 0);
        let remote_chunk = ChunkCoord::new(4, 0);
        let covered = resident_prop(&mut app, Vec3::new(32.0, 0.0, 32.0));
        let old_neighbor = resident_prop(&mut app, Vec3::new(8.0, 0.0, 32.0));
        let remote = resident_prop(&mut app, Vec3::new(288.0, 0.0, 32.0));
        let square = app.world_mut().spawn(square_at(32.0)).id();
        app.update();

        assert!(app.world().get_entity(covered).is_err());
        assert!(app.world().get_entity(old_neighbor).is_ok());
        assert!(app
            .world()
            .resource::<LoadedPropChunks>()
            .chunks
            .contains(&old_chunk));
        assert!(point_in_any_build_zone_entries(
            Vec2::splat(32.0),
            &app.world().resource::<BuildZoneChunkIndex>().by_chunk[&old_chunk],
        ));

        // Keep the owning entity: this must work without a building despawn
        // accidentally triggering the older global prop invalidation path.
        app.world_mut()
            .entity_mut(square)
            .remove::<SettlementCivicSquare>();
        app.update();

        let loaded = &app.world().resource::<LoadedPropChunks>().chunks;
        assert!(
            loaded.contains(&old_chunk),
            "released ground retains resident scenery while refilling"
        );
        assert!(loaded.contains(&remote_chunk));
        assert!(app.world().get_entity(old_neighbor).is_ok());
        assert!(app.world().get_entity(remote).is_ok());
        assert!(!app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .by_chunk
            .contains_key(&old_chunk));
        let pending = &app.world().resource::<PendingPropSpawns>().queue;
        assert!(!pending.iter().any(|(coord, _)| *coord == old_chunk));
        assert!(pending.iter().any(|(coord, _)| *coord == remote_chunk));
        assert!(app.world().get_entity(square).is_ok());
    }

    #[test]
    fn civic_square_move_clears_new_ground_and_releases_old_ground_without_reloading_neighbors() {
        let mut app = square_app();
        let old_chunk = ChunkCoord::new(0, 0);
        let new_chunk = ChunkCoord::new(1, 0);
        let old_neighbor = resident_prop(&mut app, Vec3::new(8.0, 0.0, 32.0));
        let newly_covered = resident_prop(&mut app, Vec3::new(96.0, 0.0, 32.0));
        let new_neighbor = resident_prop(&mut app, Vec3::new(72.0, 0.0, 32.0));
        let square = app.world_mut().spawn(square_at(32.0)).id();
        app.update();

        // Re-replicating an identical footprint must not make its neighbors pop.
        app.world_mut().entity_mut(square).insert(square_at(32.0));
        app.update();
        assert!(app.world().get_entity(old_neighbor).is_ok());
        assert!(app
            .world()
            .resource::<LoadedPropChunks>()
            .chunks
            .contains(&old_chunk));

        app.world_mut().entity_mut(square).insert(square_at(96.0));
        app.update();
        let loaded = &app.world().resource::<LoadedPropChunks>().chunks;
        assert!(loaded.contains(&old_chunk));
        assert!(
            loaded.contains(&new_chunk),
            "new footprint uses narrow culling, not chunk reload"
        );
        assert!(app.world().get_entity(old_neighbor).is_ok());
        assert!(app.world().get_entity(newly_covered).is_err());
        assert!(app.world().get_entity(new_neighbor).is_ok());
        assert!(!app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .by_chunk
            .contains_key(&old_chunk));
        assert!(point_in_any_build_zone_entries(
            Vec2::new(96.0, 32.0),
            &app.world().resource::<BuildZoneChunkIndex>().by_chunk[&new_chunk],
        ));
    }
}
