//! Entity-light, GPU-instanced 3D ground cover.
//!
//! This remains parallel to [`super::ground_cover`]. It uses the same
//! deterministic spawn generator, authored short/tall meshes, PBR texture,
//! climate tint and wind field, but stores sector transforms in compact GPU
//! instance buffers instead of creating an ECS entity per tuft.

use bevy::light::NotShadowCaster;
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::point_in_any_build_zone_entries;
use shared::components::VillageRoad;
use shared::props::{PropKind, PropSpawn};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};

use crate::render::systems::{ClientWorldRoot, GraphicsSettings, GroundCoverRenderer};
use crate::streaming::{streaming_anchor, AnchorCamera, AnchorPlayer};
use crate::terrain::LoadedChunks;

use super::foliage::flatten_base;
use super::ground_cover::GroundCoverStressDensity;
use super::ground_cover_instancing::{
    GrassInstance, GrassInstances, InstancedGrassExtension, InstancedGrassMaterial,
};
use super::wind::wind_params_for_mesh;
use super::{BuildZoneChunkIndex, PropAssets};

const GROUND_COVER_CHUNK_RADIUS: i32 = 4;
const GRASS_BATCH_CHUNKS: i32 = 3;

#[derive(Component)]
pub struct ChunkedGroundCover;

#[derive(Default)]
struct ChunkGrassInstances {
    short: Vec<GrassInstance>,
    tall: Vec<GrassInstance>,
}

impl ChunkGrassInstances {
    fn for_kind(&self, kind: PropKind) -> &[GrassInstance] {
        match kind {
            PropKind::GrassShortA => &self.short,
            PropKind::GrassTallA => &self.tall,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct GrassBatchKey {
    x: i32,
    z: i32,
    kind: PropKind,
}

fn batch_coord(coord: ChunkCoord) -> (i32, i32) {
    (
        coord.x.div_euclid(GRASS_BATCH_CHUNKS),
        coord.z.div_euclid(GRASS_BATCH_CHUNKS),
    )
}

#[derive(Resource, Default)]
pub struct ChunkedGroundCoverState {
    chunks: HashMap<ChunkCoord, ChunkGrassInstances>,
    render_entities: HashMap<GrassBatchKey, Entity>,
    dirty: HashSet<ChunkCoord>,
    materials: HashMap<PropKind, Handle<InstancedGrassMaterial>>,
    material_cutout: Option<bool>,
    reported_chunk_count: usize,
}

fn in_radius(coord: ChunkCoord, anchor: ChunkCoord, radius: i32) -> bool {
    (coord.x - anchor.x).abs() <= radius && (coord.z - anchor.z).abs() <= radius
}

fn grass_kinds() -> [PropKind; 2] {
    [PropKind::GrassShortA, PropKind::GrassTallA]
}

fn climate_params(terrain: &WorldTerrain) -> (f32, f32) {
    let bounds = terrain.generator.active_map_bounds();
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map(|generated| generated.seed)
        .unwrap_or(0);
    (
        (bounds.max[0] - bounds.min[0]) * 0.5,
        shared::worldgen::climate_phase(seed),
    )
}

fn stable_height(position: Vec3) -> f32 {
    let value = (position.x * 127.1 + position.z * 311.7).sin() * 43_758.547;
    let random = value - value.floor();
    1.3 * (1.0 + (random - 0.5) * 0.5)
}

fn instance_for(spawn: &PropSpawn, terrain: &WorldTerrain) -> GrassInstance {
    let (yaw, _, _) = spawn.rotation.to_euler(EulerRot::YXZ);
    GrassInstance {
        position_height: [
            spawn.position.x,
            terrain.get_height(spawn.position.x, spawn.position.z),
            spawn.position.z,
            stable_height(spawn.position),
        ],
        rotation_scale: [yaw.sin(), yaw.cos(), spawn.scale, 0.0],
    }
}

#[allow(clippy::too_many_arguments)]
fn material_for_kind(
    kind: PropKind,
    settings: &GraphicsSettings,
    terrain: &WorldTerrain,
    prop_assets: &PropAssets,
    source_meshes: &Assets<Mesh>,
    standard_materials: &Assets<StandardMaterial>,
    materials: &mut Assets<InstancedGrassMaterial>,
    state: &mut ChunkedGroundCoverState,
) -> Option<Handle<InstancedGrassMaterial>> {
    if let Some(handle) = state.materials.get(&kind) {
        return Some(handle.clone());
    }
    let set = prop_assets.tree_meshes.get(&kind)?;
    let source = source_meshes.get(&set.lod0)?;
    let mut base = standard_materials.get(&set.material)?.clone();
    flatten_base(&mut base);
    base.cull_mode = None;
    base.alpha_mode = if settings.foliage_cutout_enabled {
        AlphaMode::Mask(0.5)
    } else {
        // GPU instancing is intentionally an alpha-mask path. Preserve the
        // player's anti-aliasing preference through a softer cutoff rather
        // than routing thousands of blades through transparent sorting.
        AlphaMode::Mask(0.35)
    };
    let (climate_half, climate_phase) = climate_params(terrain);
    let handle = materials.add(ExtendedMaterial {
        base,
        extension: InstancedGrassExtension {
            params: wind_params_for_mesh(source, 0.22, 1.4),
            extra: Vec4::new(0.0, 1.0, climate_half, climate_phase),
        },
    });
    state.materials.insert(kind, handle.clone());
    Some(handle)
}

fn filtered_spawns(
    terrain: &WorldTerrain,
    coord: ChunkCoord,
    stress_density: f32,
    zones: &BuildZoneChunkIndex,
    roads: &Query<&VillageRoad>,
) -> Vec<PropSpawn> {
    let mut spawns =
        shared::props::generate_chunk_grass_at_density(&terrain.generator, coord, stress_density);
    if let Some(entries) = zones.by_chunk.get(&coord) {
        spawns.retain(|spawn| {
            !point_in_any_build_zone_entries(Vec2::new(spawn.position.x, spawn.position.z), entries)
        });
    }
    // Select roads once per chunk. A dense town can have hundreds of roads;
    // restarting the full road query for every individual tuft made a dirty
    // chunk rebuild unnecessarily quadratic in unrelated roads.
    let relevant_roads = roads
        .iter()
        .filter(|road| {
            road_chunk_bounds(road, 0.22).is_some_and(|(min, max)| {
                (min.x..=max.x).contains(&coord.x) && (min.z..=max.z).contains(&coord.z)
            })
        })
        .collect::<Vec<_>>();
    spawns.retain(|spawn| {
        !relevant_roads.iter().any(|road| {
            road.contains_built_point(Vec2::new(spawn.position.x, spawn.position.z), 0.22)
        })
    });
    spawns
}

fn clear_render_entities(commands: &mut Commands, state: &mut ChunkedGroundCoverState) {
    for (_, entity) in state.render_entities.drain() {
        commands.entity(entity).despawn();
    }
}

fn build_chunk(
    coord: ChunkCoord,
    terrain: &WorldTerrain,
    stress_density: f32,
    state: &mut ChunkedGroundCoverState,
    zones: &BuildZoneChunkIndex,
    roads: &Query<&VillageRoad>,
) {
    let spawns = filtered_spawns(terrain, coord, stress_density, zones, roads);
    let mut data = ChunkGrassInstances::default();
    for spawn in &spawns {
        match spawn.kind {
            Some(PropKind::GrassShortA) => data.short.push(instance_for(spawn, terrain)),
            Some(PropKind::GrassTallA) => data.tall.push(instance_for(spawn, terrain)),
            _ => {}
        }
    }
    state.chunks.insert(coord, data);
}

fn batch_aabb(instances: &[GrassInstance]) -> bevy::camera::primitives::Aabb {
    let first = instances[0].position_height;
    let mut min = Vec3::new(first[0], first[1], first[2]);
    let mut max = min;
    for instance in &instances[1..] {
        let position = Vec3::new(
            instance.position_height[0],
            instance.position_height[1],
            instance.position_height[2],
        );
        min = min.min(position);
        max = max.max(position);
    }
    // Covers authored blade width/height plus the strongest wind displacement.
    bevy::camera::primitives::Aabb::from_min_max(
        min - Vec3::new(3.0, 1.0, 3.0),
        max + Vec3::new(3.0, 4.0, 3.0),
    )
}

/// CPU data remains independently replaceable per terrain chunk, but nearby
/// 3×3 chunks share a render batch. This leaves only a few dozen entities while
/// retaining coarse camera culling at close zoom levels.
#[allow(clippy::too_many_arguments)]
fn sync_render_sector(
    sector: (i32, i32),
    commands: &mut Commands,
    world_root: Entity,
    settings: &GraphicsSettings,
    terrain: &WorldTerrain,
    prop_assets: &PropAssets,
    meshes: &Assets<Mesh>,
    standard_materials: &Assets<StandardMaterial>,
    materials: &mut Assets<InstancedGrassMaterial>,
    state: &mut ChunkedGroundCoverState,
) -> bool {
    for kind in grass_kinds() {
        let key = GrassBatchKey {
            x: sector.0,
            z: sector.1,
            kind,
        };
        let Some(set) = prop_assets.tree_meshes.get(&kind) else {
            return false;
        };
        if meshes.get(&set.lod0).is_none() || standard_materials.get(&set.material).is_none() {
            return false;
        }
        let instances = state
            .chunks
            .iter()
            .filter(|(coord, _)| batch_coord(**coord) == sector)
            .flat_map(|(_, chunk)| chunk.for_kind(kind).iter().copied())
            .collect::<Vec<_>>();
        if instances.is_empty() {
            if let Some(entity) = state.render_entities.remove(&key) {
                commands.entity(entity).despawn();
            }
            continue;
        }
        let aabb = batch_aabb(&instances);
        if let Some(entity) = state.render_entities.get(&key).copied() {
            commands
                .entity(entity)
                .insert((GrassInstances::new(instances), aabb));
            continue;
        }
        let Some(material) = material_for_kind(
            kind,
            settings,
            terrain,
            prop_assets,
            meshes,
            standard_materials,
            materials,
            state,
        ) else {
            return false;
        };
        let entity = commands
            .spawn((
                ChunkedGroundCover,
                Mesh3d(set.lod0.clone()),
                MeshMaterial3d(material),
                GrassInstances::new(instances),
                aabb,
                // This sector already owns the complete instance batch.
                bevy::render::batching::NoAutomaticBatching,
                Transform::IDENTITY,
                GlobalTransform::default(),
                Visibility::Visible,
                InheritedVisibility::default(),
                NotShadowCaster,
            ))
            .id();
        commands.entity(world_root).add_child(entity);
        state.render_entities.insert(key, entity);
    }
    true
}

/// Stream or rebuild at most one chunk per frame.
#[allow(clippy::too_many_arguments)]
pub(super) fn stream_chunked_ground_cover(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    anchor: (AnchorPlayer, AnchorCamera),
    prop_assets: Option<Res<PropAssets>>,
    loaded_chunks: Res<LoadedChunks>,
    meshes: Res<Assets<Mesh>>,
    standard_materials: Res<Assets<StandardMaterial>>,
    mut materials: ResMut<Assets<InstancedGrassMaterial>>,
    mut state: ResMut<ChunkedGroundCoverState>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    settings: Res<GraphicsSettings>,
    stress_density: Res<GroundCoverStressDensity>,
    zones: Res<BuildZoneChunkIndex>,
    roads: Query<&VillageRoad>,
) {
    let enabled =
        settings.props_enabled && settings.ground_cover_renderer == GroundCoverRenderer::Chunked;
    if state.material_cutout != Some(settings.foliage_cutout_enabled) {
        clear_render_entities(&mut commands, &mut state);
        state.chunks.clear();
        for (_, handle) in state.materials.drain() {
            materials.remove(handle.id());
        }
        state.material_cutout = Some(settings.foliage_cutout_enabled);
        state.reported_chunk_count = 0;
    }
    if !enabled {
        clear_render_entities(&mut commands, &mut state);
        state.chunks.clear();
        state.dirty.clear();
        state.reported_chunk_count = 0;
        return;
    }

    let (Some(terrain), Some(prop_assets)) = (terrain, prop_assets) else {
        return;
    };
    let (players, cameras) = anchor;
    let Some(anchor_pos) = streaming_anchor(&players, &cameras) else {
        return;
    };
    let Ok(world_root) = world_root_query.single() else {
        return;
    };
    let anchor_chunk = ChunkCoord::from_world_pos(anchor_pos);

    let stale = state
        .chunks
        .keys()
        .copied()
        .filter(|coord| {
            !loaded_chunks.chunks.contains(coord)
                || !in_radius(*coord, anchor_chunk, GROUND_COVER_CHUNK_RADIUS)
        })
        .collect::<Vec<_>>();
    let stale_sectors = stale
        .iter()
        .copied()
        .map(batch_coord)
        .collect::<HashSet<_>>();
    for coord in stale {
        state.chunks.remove(&coord);
        state.dirty.remove(&coord);
    }
    for sector in stale_sectors {
        sync_render_sector(
            sector,
            &mut commands,
            world_root,
            &settings,
            &terrain,
            &prop_assets,
            &meshes,
            &standard_materials,
            &mut materials,
            &mut state,
        );
    }

    let mut dirty = state
        .dirty
        .iter()
        .copied()
        .filter(|coord| state.chunks.contains_key(coord))
        .collect::<Vec<_>>();
    dirty.sort_by_key(|coord| {
        (coord.x - anchor_chunk.x)
            .abs()
            .max((coord.z - anchor_chunk.z).abs())
    });
    if let Some(coord) = dirty.first().copied() {
        build_chunk(
            coord,
            &terrain,
            stress_density.0,
            &mut state,
            &zones,
            &roads,
        );
        if sync_render_sector(
            batch_coord(coord),
            &mut commands,
            world_root,
            &settings,
            &terrain,
            &prop_assets,
            &meshes,
            &standard_materials,
            &mut materials,
            &mut state,
        ) {
            state.dirty.remove(&coord);
        }
        return;
    }

    let mut desired = anchor_chunk
        .chunks_in_radius(GROUND_COVER_CHUNK_RADIUS)
        .into_iter()
        .filter(|coord| loaded_chunks.chunks.contains(coord) && !state.chunks.contains_key(coord))
        .collect::<Vec<_>>();
    desired.sort_by_key(|coord| {
        (coord.x - anchor_chunk.x)
            .abs()
            .max((coord.z - anchor_chunk.z).abs())
    });
    if let Some(coord) = desired.first().copied() {
        build_chunk(
            coord,
            &terrain,
            stress_density.0,
            &mut state,
            &zones,
            &roads,
        );
        sync_render_sector(
            batch_coord(coord),
            &mut commands,
            world_root,
            &settings,
            &terrain,
            &prop_assets,
            &meshes,
            &standard_materials,
            &mut materials,
            &mut state,
        );
    } else if !state.chunks.is_empty() && state.reported_chunk_count != state.chunks.len() {
        let instances = state
            .chunks
            .values()
            .map(|chunk| chunk.short.len() + chunk.tall.len())
            .sum::<usize>();
        info!(
            "GPU-instanced 3D grass ready: {} chunks, {} instances, {} render entities at {:.1}x stress density (the same instances would each be a legacy render entity)",
            state.chunks.len(),
            instances,
            state.render_entities.len(),
            stress_density.0,
        );
        state.reported_chunk_count = state.chunks.len();
    }
}

pub(super) fn mark_chunked_grass_dirty_for_roads(
    settings: Res<GraphicsSettings>,
    changed: Query<&VillageRoad, Changed<VillageRoad>>,
    mut state: ResMut<ChunkedGroundCoverState>,
) {
    if settings.ground_cover_renderer != GroundCoverRenderer::Chunked {
        return;
    }
    for road in changed.iter() {
        let Some((min, max)) = road_chunk_bounds(road, 0.32) else {
            continue;
        };
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                let coord = ChunkCoord::new(x, z);
                if state.chunks.contains_key(&coord) {
                    state.dirty.insert(coord);
                }
            }
        }
    }
}

pub(super) fn mark_chunked_grass_dirty_for_buildings(
    settings: Res<GraphicsSettings>,
    added: Query<
        (
            &shared::building::PlacedBuilding,
            &shared::building::BuildingPosition,
        ),
        Added<shared::building::PlacedBuilding>,
    >,
    mut state: ResMut<ChunkedGroundCoverState>,
) {
    if settings.ground_cover_renderer != GroundCoverRenderer::Chunked {
        return;
    }
    for (building, position) in added.iter() {
        for zone in shared::building::clearance_zones_for_building(
            position.0,
            building.building_type,
            building.rotation,
        ) {
            let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
            for x in min_x..=max_x {
                for z in min_z..=max_z {
                    let coord = ChunkCoord::new(x, z);
                    if state.chunks.contains_key(&coord) {
                        state.dirty.insert(coord);
                    }
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
            (min.x / CHUNK_SIZE).floor() as i32,
            (min.y / CHUNK_SIZE).floor() as i32,
        ),
        ChunkCoord::new(
            (max.x / CHUNK_SIZE).floor() as i32,
            (max.y / CHUNK_SIZE).floor() as i32,
        ),
    ))
}

pub(super) fn clear_chunked_ground_cover(
    mut commands: Commands,
    cover: Query<Entity, With<ChunkedGroundCover>>,
    mut materials: ResMut<Assets<InstancedGrassMaterial>>,
    mut state: ResMut<ChunkedGroundCoverState>,
) {
    for entity in cover.iter() {
        commands.entity(entity).despawn();
    }
    state.render_entities.clear();
    state.chunks.clear();
    for (_, material) in state.materials.drain() {
        materials.remove(material.id());
    }
    state.dirty.clear();
    state.reported_chunk_count = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_record_is_compact_and_stable() {
        assert_eq!(size_of::<GrassInstance>(), 32);
        let position = Vec3::new(12.0, 0.0, -4.0);
        assert_eq!(stable_height(position), stable_height(position));
        assert!((0.975..=1.625).contains(&stable_height(position)));
    }

    #[test]
    fn sector_partition_handles_negative_chunk_coordinates() {
        assert_eq!(batch_coord(ChunkCoord::new(0, 0)), (0, 0));
        assert_eq!(batch_coord(ChunkCoord::new(2, 2)), (0, 0));
        assert_eq!(batch_coord(ChunkCoord::new(3, 3)), (1, 1));
        assert_eq!(batch_coord(ChunkCoord::new(-1, -1)), (-1, -1));
        assert_eq!(batch_coord(ChunkCoord::new(-3, -3)), (-1, -1));
        assert_eq!(batch_coord(ChunkCoord::new(-4, -4)), (-2, -2));
    }

    #[test]
    fn batch_bounds_cover_all_instances_and_wind_margin() {
        let records = [
            GrassInstance {
                position_height: [10.0, 2.0, -4.0, 1.0],
                rotation_scale: [0.0; 4],
            },
            GrassInstance {
                position_height: [18.0, 5.0, 7.0, 1.0],
                rotation_scale: [0.0; 4],
            },
        ];
        let bounds = batch_aabb(&records);
        let min = Vec3::from(bounds.min());
        let max = Vec3::from(bounds.max());
        assert!(min.cmple(Vec3::new(7.0, 1.0, -7.0)).all());
        assert!(max.cmpge(Vec3::new(21.0, 9.0, 10.0)).all());
    }
}
