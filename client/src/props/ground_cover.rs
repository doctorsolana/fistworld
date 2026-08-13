//! Ground cover streams on its own, much tighter radius than the props.
//!
//! Trees have to exist far out — they are the silhouette of the landscape, and
//! the prop radius grows with zoom until it reaches 512 m. Grass is the
//! opposite: it carries an 80 m `visible_end_distance`, so anything spawned
//! beyond that ring is an entity, a transform and a visibility check that can
//! never draw a pixel. At full zoom the shared radius would have spawned tens
//! of thousands of them.
//!
//! So ground cover gets its own loaded-set, its own index and its own radius,
//! and the two layers stream past each other without interfering. The cost of
//! the split is this file; the cost of not splitting it is a frame budget.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::point_in_any_build_zone_entries;
use shared::components::VillageRoad;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::render::systems::{ClientWorldRoot, GraphicsSettings, GroundCoverRenderer};
use crate::streaming::{streaming_anchor, AnchorCamera, AnchorPlayer};
use crate::terrain::LoadedChunks;

use super::spawn::spawn_prop_instance;
use super::{PropAssets, SimplePropMeshCache};

/// How far ground cover streams, in chunks.
///
/// Four chunks is 256 m, which covers the 240 m draw distance with a chunk to
/// spare so a patch is loaded before it could be seen. Deliberately NOT scaled
/// with zoom the way the prop radius is: zooming out does not make grass
/// visible further away, it only makes it smaller.
///
/// The budget behind the number, measured rather than guessed: 278 patches per
/// chunk in temperate meadow (50 in the north, 54 in the desert -- the farmland
/// gradient), so 81 chunks is usually about 16,000 patches, one entity each.
const GROUND_COVER_CHUNK_RADIUS: i32 = 4;

/// Spawn budget per frame. Higher than the prop budget because there are two
/// orders of magnitude more patches than trees and they are one cheap entity
/// each; at 64 a frame filling the ring took nine seconds of visible growing-in.
const MAX_GROUND_COVER_SPAWNS_PER_FRAME: usize = 256;

/// Marks an entity as ground cover so it can be culled on its own radius.
#[derive(Component)]
pub struct GroundCover;

#[derive(Resource, Default)]
pub struct LoadedGroundCoverChunks {
    pub chunks: HashSet<ChunkCoord>,
}

#[derive(Resource, Default)]
pub struct GroundCoverIndex {
    pub by_chunk: HashMap<ChunkCoord, Vec<Entity>>,
}

#[derive(Resource, Default)]
pub struct PendingGroundCover {
    pub queue: std::collections::VecDeque<(ChunkCoord, Vec<shared::props::PropSpawn>)>,
    reported_entities: usize,
}

/// Opt-in renderer stress input. This is deliberately not a saved graphics
/// setting: ordinary worlds remain at 1x, while captures can request a much
/// denser meadow with `FISTFORCE_GRASS_STRESS_DENSITY`.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GroundCoverStressDensity(pub f32);

impl Default for GroundCoverStressDensity {
    fn default() -> Self {
        let multiplier = std::env::var("FISTFORCE_GRASS_STRESS_DENSITY")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(1.0)
            .clamp(1.0, 32.0);
        Self(multiplier)
    }
}

fn in_radius(coord: ChunkCoord, anchor: ChunkCoord, radius: i32) -> bool {
    (coord.x - anchor.x).abs() <= radius && (coord.z - anchor.z).abs() <= radius
}

/// Grow ground cover for chunks near the anchor.
#[allow(clippy::too_many_arguments)]
pub(super) fn stream_ground_cover(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    anchor: (AnchorPlayer, AnchorCamera),
    prop_assets: Option<Res<PropAssets>>,
    mut simple_mesh_cache: ResMut<SimplePropMeshCache>,
    gltf_assets: (
        Option<Res<Assets<bevy::gltf::Gltf>>>,
        Option<Res<Assets<bevy::gltf::GltfNode>>>,
        Option<Res<Assets<bevy::gltf::GltfMesh>>>,
    ),
    loaded_chunks: Res<LoadedChunks>,
    mut loaded: ResMut<LoadedGroundCoverChunks>,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    settings: Res<GraphicsSettings>,
    stress_density: Res<GroundCoverStressDensity>,
    build_zone_index: Res<super::BuildZoneChunkIndex>,
    roads: Query<&VillageRoad>,
) {
    let Some(terrain) = terrain else { return };
    let (player_query, camera_query) = anchor;
    let (gltfs, gltf_nodes, gltf_meshes) = gltf_assets;
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
    if settings.ground_cover_renderer != GroundCoverRenderer::Legacy {
        return;
    }

    let anchor_chunk = ChunkCoord::from_world_pos(anchor_pos);

    // Stage 1: one chunk's worth of cover queued per frame, nearest first.
    let mut desired: Vec<ChunkCoord> = anchor_chunk
        .chunks_in_radius(GROUND_COVER_CHUNK_RADIUS)
        .into_iter()
        .filter(|coord| loaded_chunks.chunks.contains(coord) && !loaded.chunks.contains(coord))
        .collect();
    desired.sort_by_key(|coord| {
        (coord.x - anchor_chunk.x)
            .abs()
            .max((coord.z - anchor_chunk.z).abs())
    });

    if let Some(coord) = desired.first().copied() {
        let mut spawns = shared::props::generate_chunk_grass_at_density(
            &terrain.generator,
            coord,
            stress_density.0,
        );
        // Ground cover respects build zones exactly as the props do. It is easy
        // to forget precisely because grass is generated rather than authored --
        // it never passed through the prop spawner, so it never inherited the
        // filter, and grass would have grown through floorboards.
        if let Some(zones) = build_zone_index.by_chunk.get(&coord) {
            spawns.retain(|spawn| {
                !point_in_any_build_zone_entries(
                    Vec2::new(spawn.position.x, spawn.position.z),
                    zones,
                )
            });
        }
        spawns.retain(|spawn| {
            !roads.iter().any(|road| {
                road.contains_built_point(Vec2::new(spawn.position.x, spawn.position.z), 0.22)
            })
        });
        loaded.chunks.insert(coord);
        if !spawns.is_empty() {
            pending.queue.push_back((coord, spawns));
        }
    }

    // Stage 2: realise a bounded number of queued patches.
    let mut budget = ((MAX_GROUND_COVER_SPAWNS_PER_FRAME as f32 * stress_density.0).ceil()
        as usize)
        .min(MAX_GROUND_COVER_SPAWNS_PER_FRAME * 32);
    while budget > 0 {
        let Some((coord, spawns)) = pending.queue.front_mut() else {
            break;
        };
        let coord = *coord;
        let take = budget.min(spawns.len());
        for spawn in spawns.drain(..take) {
            let entity = spawn_prop_instance(
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
            commands.entity(entity).insert(GroundCover);
            index.by_chunk.entry(coord).or_default().push(entity);
        }
        budget -= take;
        if pending.queue.front().is_some_and(|(_, s)| s.is_empty()) {
            pending.queue.pop_front();
        }
    }

    if desired.is_empty() && pending.queue.is_empty() {
        let entities = index.by_chunk.values().map(Vec::len).sum::<usize>();
        if entities > 0 && pending.reported_entities != entities {
            info!(
                "Legacy 3D grass ready: {} chunks, {} render entities at {:.1}x stress density",
                loaded.chunks.len(),
                entities,
                stress_density.0,
            );
            pending.reported_entities = entities;
        }
    }
}

/// Trample grass as a path grows, without unloading or blinking whole chunks.
pub(super) fn clear_ground_cover_for_built_village_roads(
    mut commands: Commands,
    changed_roads: Query<&VillageRoad, Changed<VillageRoad>>,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
    transforms: Query<&GlobalTransform, With<GroundCover>>,
    settings: Res<GraphicsSettings>,
) {
    if settings.ground_cover_renderer != GroundCoverRenderer::Legacy {
        return;
    }
    for road in changed_roads.iter() {
        let Some((min_chunk, max_chunk)) = road_chunk_bounds(road, 0.32) else {
            continue;
        };
        for cx in min_chunk.x..=max_chunk.x {
            for cz in min_chunk.z..=max_chunk.z {
                let coord = ChunkCoord::new(cx, cz);
                if let Some(entities) = index.by_chunk.get_mut(&coord) {
                    entities.retain(|entity| {
                        let Ok(transform) = transforms.get(*entity) else {
                            return true;
                        };
                        let at = transform.translation();
                        if road.contains_built_point(Vec2::new(at.x, at.z), 0.22) {
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
                        !road.contains_built_point(
                            Vec2::new(spawn.position.x, spawn.position.z),
                            0.22,
                        )
                    });
                }
            }
        }
    }
}

fn road_chunk_bounds(road: &VillageRoad, padding: f32) -> Option<(ChunkCoord, ChunkCoord)> {
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
        ChunkCoord::new(
            (min.x / shared::terrain::CHUNK_SIZE).floor() as i32,
            (min.y / shared::terrain::CHUNK_SIZE).floor() as i32,
        ),
        ChunkCoord::new(
            (max.x / shared::terrain::CHUNK_SIZE).floor() as i32,
            (max.y / shared::terrain::CHUNK_SIZE).floor() as i32,
        ),
    ))
}

/// Re-grow a chunk's cover when a building claims ground in it.
///
/// The prop streamer has had this since buildings existed; ground cover needed
/// its own because it keeps its own loaded-set. Without it, grass already
/// standing when the plot was claimed keeps standing -- inside the building.
pub(super) fn clear_ground_cover_for_new_buildings(
    mut commands: Commands,
    added: Query<
        (
            &shared::building::PlacedBuilding,
            &shared::building::BuildingPosition,
        ),
        Added<shared::building::PlacedBuilding>,
    >,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
    transforms: Query<&GlobalTransform>,
    settings: Res<GraphicsSettings>,
) {
    if settings.ground_cover_renderer != GroundCoverRenderer::Legacy {
        return;
    }
    for (building, position) in added.iter() {
        let zones = shared::building::clearance_zones_for_building(
            position.0,
            building.building_type,
            building.rotation,
        );
        // Only the patches ON the plot, for the same reason the props are
        // culled surgically: re-growing whole chunks makes the whole meadow
        // blink every time a hut goes up.
        for zone in zones {
            let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
            for cx in min_x..=max_x {
                for cz in min_z..=max_z {
                    let coord = ChunkCoord::new(cx, cz);
                    if let Some(entities) = index.by_chunk.get_mut(&coord) {
                        entities.retain(|patch| {
                            let Ok(transform) = transforms.get(*patch) else {
                                return true;
                            };
                            let at = transform.translation();
                            if zone.contains_point(Vec2::new(at.x, at.z)) {
                                commands.entity(*patch).despawn();
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
                            !zone.contains_point(Vec2::new(spawn.position.x, spawn.position.z))
                        });
                    }
                }
            }
        }
    }
}

/// Tear down the retained patch renderer immediately when the player selects
/// the chunked renderer. Its resources stay initialized so switching back is
/// a normal stream-in, not a restart.
pub(super) fn sync_legacy_ground_cover_mode(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    cover: Query<Entity, With<GroundCover>>,
    mut loaded: ResMut<LoadedGroundCoverChunks>,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
) {
    if settings.ground_cover_renderer == GroundCoverRenderer::Legacy {
        return;
    }
    for entity in cover.iter() {
        commands.entity(entity).despawn();
    }
    loaded.chunks.clear();
    pending.queue.clear();
    pending.reported_entities = 0;
    index.by_chunk.clear();
}

/// Drop ground cover once its chunk leaves the ring.
pub(super) fn cleanup_ground_cover(
    mut commands: Commands,
    player_query: AnchorPlayer,
    camera_query: AnchorCamera,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded: ResMut<LoadedGroundCoverChunks>,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
) {
    let anchor_chunk =
        streaming_anchor(&player_query, &camera_query).map(ChunkCoord::from_world_pos);

    let stale: Vec<ChunkCoord> = loaded
        .chunks
        .iter()
        .copied()
        .filter(|coord| {
            if !loaded_chunks.chunks.contains(coord) {
                return true;
            }
            let Some(anchor_chunk) = anchor_chunk else {
                return false;
            };
            !in_radius(*coord, anchor_chunk, GROUND_COVER_CHUNK_RADIUS)
        })
        .collect();

    for coord in stale {
        if let Some(entities) = index.by_chunk.remove(&coord) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        pending.queue.retain(|(c, _)| *c != coord);
        loaded.chunks.remove(&coord);
    }
}

/// Forget everything on world teardown, so a reconnect does not inherit ghosts.
pub(super) fn clear_ground_cover(
    mut commands: Commands,
    cover: Query<Entity, With<GroundCover>>,
    mut loaded: ResMut<LoadedGroundCoverChunks>,
    mut pending: ResMut<PendingGroundCover>,
    mut index: ResMut<GroundCoverIndex>,
) {
    for entity in cover.iter() {
        commands.entity(entity).despawn();
    }
    loaded.chunks.clear();
    pending.queue.clear();
    index.by_chunk.clear();
}
