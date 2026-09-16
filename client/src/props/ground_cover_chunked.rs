//! Entity-light, GPU-instanced 3D ground cover.
//!
//! The deterministic spawn generator feeds compact GPU instance buffers grouped
//! into camera-cullable sectors instead of creating an ECS entity per tuft.

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::building::{point_in_any_build_zone_entries, BuildZoneEntry};
use shared::components::{distance_squared_to_segment, VillageRoad};
use shared::props::{PropKind, PropSpawn};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};

use crate::render::systems::{ClientWorldRoot, GraphicsSettings};
use crate::streaming::{
    camera_view_distance, chunk_stream_priority, streaming_anchor, streaming_view_priority,
    AnchorCamera, AnchorPlayer,
};
use crate::terrain::{LoadedChunks, TerrainChunk};

use super::foliage::flatten_base;
use super::ground_cover_instancing::{
    GrassInstance, GrassInstances, InstancedGrassExtension, InstancedGrassMaterial,
};
use super::wind::wind_params_for_mesh;
use super::{BuildZoneChunkIndex, PropAssets};

const GROUND_COVER_CHUNK_RADIUS: i32 = 4;
const GRASS_BATCH_CHUNKS: i32 = 3;
/// Trampled/cut verges transition back into the taller wild meadow. This is
/// presentation only; the server's road and navigation footprint is unchanged.
const TENDED_ROAD_VERGE: f32 = 11.0;
/// Grass blades are well below one pixel by this distance. Keeping their
/// alpha-masked geometry alive beyond it turns whole meadow sectors into
/// shimmering dots and wastes fill rate in middle/map view.
const GROUND_COVER_END_DISTANCE: f32 = 700.0;
const _: () = assert!(GROUND_COVER_END_DISTANCE < crate::terrain::map_view::MAP_VIEW_BLEND_START);

fn ground_cover_visibility_range() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        // Abrupt on purpose: the custom instanced shader does not participate
        // in Bevy's dither crossfade, and dithering tiny blades recreates the
        // speckle this cutoff is intended to remove.
        end_margin: GROUND_COVER_END_DISTANCE..GROUND_COVER_END_DISTANCE,
        use_aabb: true,
    }
}

/// Opt-in renderer stress input. Ordinary worlds remain at 1x, while captures
/// can request denser meadows with `FISTFORCE_GRASS_STRESS_DENSITY`.
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

#[derive(Component)]
pub struct ChunkedGroundCover;

#[derive(Default)]
struct ChunkGrassInstances {
    field_revision: u64,
    building_zones: Vec<BuildZoneEntry>,
    terrain_mesh: Option<bevy::asset::AssetId<Mesh>>,
    short: Vec<GrassInstance>,
    tall: Vec<GrassInstance>,
    fern_a: Vec<GrassInstance>,
    fern_b: Vec<GrassInstance>,
    flowers: [Vec<GrassInstance>; 4],
}

impl ChunkGrassInstances {
    fn for_kind(&self, kind: PropKind) -> &[GrassInstance] {
        match kind {
            PropKind::GrassShortA => &self.short,
            PropKind::GrassTallA => &self.tall,
            PropKind::FernPatchA => &self.fern_a,
            PropKind::FernPatchB => &self.fern_b,
            PropKind::FlowerA => &self.flowers[0],
            PropKind::FlowerB => &self.flowers[1],
            PropKind::FlowerC => &self.flowers[2],
            PropKind::FlowerD => &self.flowers[3],
            _ => &[],
        }
    }

    fn for_kind_mut(&mut self, kind: PropKind) -> Option<&mut Vec<GrassInstance>> {
        match kind {
            PropKind::GrassShortA => Some(&mut self.short),
            PropKind::GrassTallA => Some(&mut self.tall),
            PropKind::FernPatchA => Some(&mut self.fern_a),
            PropKind::FernPatchB => Some(&mut self.fern_b),
            PropKind::FlowerA => Some(&mut self.flowers[0]),
            PropKind::FlowerB => Some(&mut self.flowers[1]),
            PropKind::FlowerC => Some(&mut self.flowers[2]),
            PropKind::FlowerD => Some(&mut self.flowers[3]),
            _ => None,
        }
    }

    fn instance_count(&self) -> usize {
        ground_cover_kinds()
            .iter()
            .map(|kind| self.for_kind(*kind).len())
            .sum()
    }
}

/// Flower patches are authored colonies like ferns: no per-tuft height
/// stretch, no dryness tint, a gentle nod instead of the grass sway.
fn is_flower_patch(kind: PropKind) -> bool {
    matches!(
        kind,
        PropKind::FlowerA | PropKind::FlowerB | PropKind::FlowerC | PropKind::FlowerD
    )
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
    yard_version: u64,
    road_bounds: HashMap<Entity, (ChunkCoord, ChunkCoord)>,
    roads_initialized: bool,
}

fn in_radius(coord: ChunkCoord, anchor: ChunkCoord, radius: i32) -> bool {
    (coord.x - anchor.x).abs() <= radius && (coord.z - anchor.z).abs() <= radius
}

/// Every kind the ground-cover generator can emit, each with its own instance
/// buffer per render sector. All of them need a `tree_mesh_labels` entry
/// (`material_for_kind` reads the LOD0 mesh and material from it).
fn ground_cover_kinds() -> [PropKind; 8] {
    [
        PropKind::GrassShortA,
        PropKind::GrassTallA,
        PropKind::FernPatchA,
        PropKind::FernPatchB,
        PropKind::FlowerA,
        PropKind::FlowerB,
        PropKind::FlowerC,
        PropKind::FlowerD,
    ]
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

fn meadow_hash(x: u32, z: u32, seed: u64) -> f32 {
    let value = shared::worldgen::splitmix64(((x as u64) << 32) ^ z as u64 ^ seed);
    (value >> 40) as f32 / (1_u32 << 24) as f32
}

fn smooth_unit(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smooth_meadow_noise(point: Vec2, seed: u64) -> f32 {
    let cell = point.floor().as_ivec2();
    let fraction = point - point.floor();
    let x = smooth_unit(fraction.x);
    let z = smooth_unit(fraction.y);
    let sample = |dx: i32, dz: i32| meadow_hash((cell.x + dx) as u32, (cell.y + dz) as u32, seed);
    let corners = [sample(0, 0), sample(1, 0), sample(0, 1), sample(1, 1)];
    let low = corners[0] + (corners[1] - corners[0]) * x;
    let high = corners[2] + (corners[3] - corners[2]) * x;
    low + (high - low) * z
}

/// Broad, smooth tending patches with no chunk-local seed or repeating tile.
/// Differently rotated 16/29 m fields avoid an axis-aligned mown checkerboard.
fn tended_meadow_patch(point: Vec2, seed: u64) -> f32 {
    let fine = Vec2::new(
        point.x * 0.924 + point.y * 0.383,
        -point.x * 0.383 + point.y * 0.924,
    ) / 16.0;
    let broad = Vec2::new(point.x * 0.6 - point.y * 0.8, point.x * 0.8 + point.y * 0.6) / 29.0;
    let value = smooth_meadow_noise(fine, seed ^ 0x51AF_2039) * 0.72
        + smooth_meadow_noise(broad, seed ^ 0xC936_72A1) * 0.28;
    smooth_unit((value - 0.40) / 0.25)
}

fn tended_grass_retention(point: Vec2, seed: u64, road_edge_distance: f32, tall: bool) -> f32 {
    if road_edge_distance >= TENDED_ROAD_VERGE {
        return 1.0;
    }
    // Keep some taller tufts in the quieter patches: a verge should read as
    // irregular groups, not a uniformly shaved lawn. Size reduction already
    // happens below, so this does not add another height multiplier.
    let influence = 1.0 - smooth_unit((road_edge_distance - 3.5) / (TENDED_ROAD_VERGE - 3.5));
    let floor: f32 = if tall { 0.50 } else { 0.22 };
    let patch_retention = floor + (1.0 - floor) * tended_meadow_patch(point, seed);
    1.0 - influence * (1.0 - patch_retention)
}

fn instance_for(
    spawn: &PropSpawn,
    terrain: &WorldTerrain,
    dryness: &shared::props::MeadowDryness,
) -> GrassInstance {
    let (yaw, _, _) = spawn.rotation.to_euler(EulerRot::YXZ);
    // Per-tuft dryness: the shared field says WHERE the meadow runs dry
    // (agreeing with the spawn thinning and the ground mottle), a stable
    // per-tuft jitter keeps neighbours from being clones. Rides the
    // previously unused .w lane - no stride change, no pipeline change.
    let jitter = {
        let value = (spawn.position.x * 419.3 + spawn.position.z * 173.7).sin() * 43_758.547;
        value - value.floor()
    };
    let authored_colony = spawn.kind.is_some_and(|kind| {
        matches!(kind, PropKind::FernPatchA | PropKind::FernPatchB) || is_flower_patch(kind)
    });
    let dry = if authored_colony {
        0.0
    } else {
        (dryness.sample(spawn.position.x, spawn.position.z) + (jitter - 0.5) * 0.35).clamp(0.0, 1.0)
    };
    GrassInstance {
        position_height: [
            spawn.position.x,
            terrain.get_height(spawn.position.x, spawn.position.z),
            spawn.position.z,
            // Grass benefits from per-tuft height noise. A fern or flower
            // asset is an authored colony with a deliberate silhouette;
            // stretching only its vertical axis turns it into a spiky grass
            // tuft again (or lifts the blossoms off their stems).
            if authored_colony {
                1.0
            } else {
                stable_height(spawn.position)
            },
        ],
        rotation_scale: [yaw.sin(), yaw.cos(), spawn.scale, dry],
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
    let (sway_strength, flutter_rate) =
        if matches!(kind, PropKind::FernPatchA | PropKind::FernPatchB) {
            (0.055, 1.35)
        } else if is_flower_patch(kind) {
            (0.06, 1.3)
        } else {
            (0.22, 1.4)
        };
    // extra.x = 1 tells instanced_grass.wgsl to keep the authored vertex
    // colours instead of applying the per-tuft dryness tint (white petals
    // would otherwise go straw-yellow on dry ground).
    let keep_authored_colour = if is_flower_patch(kind) { 1.0 } else { 0.0 };
    let handle = materials.add(ExtendedMaterial {
        base,
        extension: InstancedGrassExtension {
            params: wind_params_for_mesh(source, sway_strength, flutter_rate),
            extra: Vec4::new(keep_authored_colour, 1.0, climate_half, climate_phase),
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
    yards: Option<&crate::settlement::yards::YardGroundCover>,
) -> Vec<PropSpawn> {
    let mut spawns =
        shared::props::generate_chunk_grass_at_density(&terrain.generator, coord, stress_density);
    if let Some(entries) = zones.by_chunk.get(&coord) {
        spawns.retain(|spawn| {
            !point_in_any_build_zone_entries(Vec2::new(spawn.position.x, spawn.position.z), entries)
        });
    }
    spawns.retain(|spawn| !zones.fields.contains_point(spawn.position.xz()));
    // Select roads once per chunk. A dense town can have hundreds of roads;
    // restarting the full road query for every individual tuft made a dirty
    // chunk rebuild unnecessarily quadratic in unrelated roads.
    let relevant_roads = roads
        .iter()
        .filter(|road| {
            road_chunk_bounds(road, TENDED_ROAD_VERGE).is_some_and(|(min, max)| {
                (min.x..=max.x).contains(&coord.x) && (min.z..=max.z).contains(&coord.z)
            })
        })
        .collect::<Vec<_>>();
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map_or(0, |generated| generated.seed);
    spawns.retain_mut(|spawn| {
        let point = spawn.position.xz();
        let road_edge_distance = relevant_roads
            .iter()
            .map(|road| {
                let distance_squared = road
                    .built_points()
                    .windows(2)
                    .map(|pair| distance_squared_to_segment(point, pair[0], pair[1]))
                    .fold(f32::INFINITY, f32::min);
                distance_squared.sqrt() - road.width * 0.5
            })
            .fold(f32::INFINITY, f32::min);
        if road_edge_distance <= 0.22
            || yards.is_some_and(|yards| yards.contains_world_point(point))
        {
            return false;
        }
        verge_keeps(spawn, road_edge_distance, seed)
    });
    spawns
}

/// The per-spawn verge rule once a spawn is known to be clear of roads,
/// yards, fields and build zones: false drops it, and a grass tuft kept
/// inside the tended verge is shrunk as well.
fn verge_keeps(spawn: &mut PropSpawn, road_edge_distance: f32, seed: u64) -> bool {
    let point = spawn.position.xz();
    // The tended verge is trampled, cut ground: no wild flower drifts
    // there. The roadside dressing plants its own bounded meadow clusters
    // 8-20 m out (settlement/roadside/placement.rs), so the two layers
    // overlap only in the 11-20 m band beyond the verge, by design.
    if spawn.kind.is_some_and(is_flower_patch) {
        return road_edge_distance >= TENDED_ROAD_VERGE;
    }
    if !matches!(
        spawn.kind,
        Some(PropKind::GrassShortA | PropKind::GrassTallA)
    ) {
        return true;
    }
    if road_edge_distance < TENDED_ROAD_VERGE {
        let retention = tended_grass_retention(
            point,
            seed,
            road_edge_distance,
            spawn.kind == Some(PropKind::GrassTallA),
        );
        let roll = meadow_hash(point.x.to_bits(), point.y.to_bits(), seed ^ 0x7D32_84E9);
        if roll >= retention {
            return false;
        }
        spawn.scale *= if road_edge_distance <= 3.0 {
            0.62
        } else {
            0.82
        };
    }
    true
}

fn clear_render_entities(commands: &mut Commands, state: &mut ChunkedGroundCoverState) {
    for (_, entity) in state.render_entities.drain() {
        commands.entity(entity).despawn();
    }
}

fn build_chunk(
    coord: ChunkCoord,
    terrain_mesh: bevy::asset::AssetId<Mesh>,
    terrain: &WorldTerrain,
    stress_density: f32,
    state: &mut ChunkedGroundCoverState,
    zones: &BuildZoneChunkIndex,
    roads: &Query<&VillageRoad>,
    yards: Option<&crate::settlement::yards::YardGroundCover>,
) {
    let spawns = filtered_spawns(terrain, coord, stress_density, zones, roads, yards);
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map(|generated| generated.seed)
        .unwrap_or(0);
    let dryness = shared::props::MeadowDryness::new(seed);
    let mut data = ChunkGrassInstances {
        field_revision: zones.fields.chunk_revision(coord),
        building_zones: zones.by_chunk.get(&coord).cloned().unwrap_or_default(),
        terrain_mesh: Some(terrain_mesh),
        ..default()
    };
    for spawn in &spawns {
        if let Some(buffer) = spawn.kind.and_then(|kind| data.for_kind_mut(kind)) {
            buffer.push(instance_for(spawn, terrain, &dryness));
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
    for kind in ground_cover_kinds() {
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
                ground_cover_visibility_range(),
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
    yards: Option<Res<crate::settlement::yards::YardGroundCover>>,
    terrain_meshes: Query<(&TerrainChunk, &Mesh3d)>,
) {
    let enabled = settings.props_enabled;
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
    if super::props_suppressed_at_zoom(camera_view_distance(&cameras)) {
        clear_render_entities(&mut commands, &mut state);
        state.chunks.clear();
        state.dirty.clear();
        state.reported_chunk_count = 0;
        return;
    }
    let Some(anchor_pos) = streaming_anchor(&players, &cameras) else {
        return;
    };
    let view_priority = streaming_view_priority(&cameras);
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

    if let Some(yards) = yards.as_deref() {
        if state.yard_version != yards.version() {
            // Only accepted land changes invalidate ground cover, never the
            // state of individual plants or household members.
            state.yard_version = yards.version();
            let loaded = state.chunks.keys().copied().collect::<Vec<_>>();
            state.dirty.extend(loaded);
        }
    }

    // The bounded resident grass set keeps per-chunk revisions. A remote field
    // edit does not rebuild nearby grass, and removals return its revision to 0.
    let changed_fields = state
        .chunks
        .iter()
        .filter_map(|(coord, data)| {
            (data.field_revision != zones.fields.chunk_revision(*coord)).then_some(*coord)
        })
        .collect::<Vec<_>>();
    state.dirty.extend(changed_fields);

    let next_dirty = state
        .dirty
        .iter()
        .copied()
        .filter(|coord| {
            state.chunks.contains_key(coord) && !loaded_chunks.rebuilding.contains(coord)
        })
        .min_by_key(|coord| chunk_stream_priority(*coord, anchor_pos, view_priority));
    if let Some(coord) = next_dirty {
        let Some(mesh) = terrain_meshes
            .iter()
            .find_map(|(chunk, mesh)| (chunk.coord == coord).then_some(mesh.id()))
        else {
            return;
        };
        build_chunk(
            coord,
            mesh,
            &terrain,
            stress_density.0,
            &mut state,
            &zones,
            &roads,
            yards.as_deref(),
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

    let next = (-GROUND_COVER_CHUNK_RADIUS..=GROUND_COVER_CHUNK_RADIUS)
        .flat_map(|dx| {
            (-GROUND_COVER_CHUNK_RADIUS..=GROUND_COVER_CHUNK_RADIUS)
                .map(move |dz| ChunkCoord::new(anchor_chunk.x + dx, anchor_chunk.z + dz))
        })
        .filter(|coord| {
            loaded_chunks.chunks.contains(coord)
                && !loaded_chunks.rebuilding.contains(coord)
                && !state.chunks.contains_key(coord)
        })
        .min_by_key(|coord| chunk_stream_priority(*coord, anchor_pos, view_priority));
    if let Some(coord) = next {
        let Some(mesh) = terrain_meshes
            .iter()
            .find_map(|(chunk, mesh)| (chunk.coord == coord).then_some(mesh.id()))
        else {
            return;
        };
        build_chunk(
            coord,
            mesh,
            &terrain,
            stress_density.0,
            &mut state,
            &zones,
            &roads,
            yards.as_deref(),
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
            .map(ChunkGrassInstances::instance_count)
            .sum::<usize>();
        let flowers = state
            .chunks
            .values()
            .map(|chunk| chunk.flowers.iter().map(Vec::len).sum::<usize>())
            .sum::<usize>();
        info!(
            "GPU-instanced 3D ground cover ready: {} chunks, {} instances ({} flower patches), {} render entities at {:.1}x stress density",
            state.chunks.len(),
            instances,
            flowers,
            state.render_entities.len(),
            stress_density.0,
        );
        state.reported_chunk_count = state.chunks.len();
    }
}

pub(super) fn mark_chunked_grass_dirty_for_roads(
    changed: Query<(Entity, &VillageRoad), Changed<VillageRoad>>,
    all: Query<(Entity, &VillageRoad)>,
    mut removed: RemovedComponents<VillageRoad>,
    mut state: ResMut<ChunkedGroundCoverState>,
) {
    let initial = !state.roads_initialized;
    state.roads_initialized = true;
    let mut dirty_bounds = Vec::new();
    for entity in removed.read() {
        if let Some(bounds) = state.road_bounds.remove(&entity) {
            dirty_bounds.push(bounds);
        }
    }
    for (entity, road) in
        changed
            .iter()
            .chain(all.iter().take(if initial { usize::MAX } else { 0 }))
    {
        if let Some(bounds) = state.road_bounds.remove(&entity) {
            dirty_bounds.push(bounds);
        }
        if let Some(bounds) = road_chunk_bounds(road, TENDED_ROAD_VERGE) {
            state.road_bounds.insert(entity, bounds);
            dirty_bounds.push(bounds);
        }
    }
    for (min, max) in dirty_bounds {
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

/// Compare the exact exclusions used by each resident chunk with the shared
/// prop index. This also catches removals, rotations, movement and the switch
/// from inferred farm clearances to accepted fields without another owner cache.
pub(super) fn mark_chunked_grass_dirty_for_buildings(
    mut state: ResMut<ChunkedGroundCoverState>,
    zones: Res<BuildZoneChunkIndex>,
) {
    if !zones.is_changed() {
        return;
    }
    let state = &mut *state;
    for (&coord, data) in &state.chunks {
        let current = zones.by_chunk.get(&coord).map_or(&[][..], Vec::as_slice);
        if data.building_zones.as_slice() != current {
            state.dirty.insert(coord);
        }
    }
}

/// Edited terrain stays resident while its replacement is built. Refresh the
/// affected grass only after that replacement exists, keeping its old render
/// batch visible until the budgeted instance-buffer update is ready.
pub(super) fn mark_chunked_grass_dirty_for_terrain(
    terrain_meshes: Query<(&TerrainChunk, &Mesh3d), Changed<Mesh3d>>,
    mut state: ResMut<ChunkedGroundCoverState>,
) {
    let state = &mut *state;
    for (chunk, mesh) in &terrain_meshes {
        let Some(data) = state.chunks.get(&chunk.coord) else {
            continue;
        };
        // PBR also marks Mesh3d changed when its material changes. Water-clock
        // and splat updates do not change the height surface or grass placement.
        if data.terrain_mesh != Some(mesh.id()) {
            state.dirty.insert(chunk.coord);
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
    state.road_bounds.clear();
    state.roads_initialized = false;
    state.reported_chunk_count = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grass_footprint_app() -> App {
        let mut app = App::new();
        app.init_resource::<ChunkedGroundCoverState>()
            .init_resource::<BuildZoneChunkIndex>()
            .init_resource::<super::super::PropFootprintSources>()
            .init_resource::<super::super::LoadedPropChunks>()
            .init_resource::<super::super::PendingPropSpawns>()
            .init_resource::<super::super::PropChunkIndex>()
            .add_systems(
                Update,
                (
                    super::super::spawn::invalidate_props_for_new_buildings,
                    super::super::spawn::sync_build_zone_chunk_index,
                    mark_chunked_grass_dirty_for_buildings,
                )
                    .chain(),
            );
        for coord in [
            ChunkCoord::new(0, 0),
            ChunkCoord::new(3, 0),
            ChunkCoord::new(8, 0),
        ] {
            app.world_mut()
                .resource_mut::<ChunkedGroundCoverState>()
                .chunks
                .insert(coord, default());
        }
        app
    }

    /// Advance the invalidation fixture to the geometry a completed grass
    /// rebuild would retain, without requiring a GPU or procedural meadow.
    fn accept_grass_footprints(app: &mut App) {
        let zones = app
            .world()
            .resource::<BuildZoneChunkIndex>()
            .by_chunk
            .clone();
        let mut state = app.world_mut().resource_mut::<ChunkedGroundCoverState>();
        for (&coord, data) in &mut state.chunks {
            data.building_zones = zones.get(&coord).cloned().unwrap_or_default();
        }
        state.dirty.clear();
    }

    #[test]
    fn building_changes_dirty_old_and_new_grass_without_resetting_surroundings() {
        use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
        let mut app = grass_footprint_app();
        let old = ChunkCoord::new(0, 0);
        let new = ChunkCoord::new(3, 0);
        let batch = app.world_mut().spawn(ChunkedGroundCover).id();
        let key = GrassBatchKey {
            x: 0,
            z: 0,
            kind: PropKind::GrassShortA,
        };
        app.world_mut()
            .resource_mut::<ChunkedGroundCoverState>()
            .render_entities
            .insert(key, batch);
        let building = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::LogCabin,
                    rotation: 0.0,
                },
                BuildingPosition(Vec3::new(32.0, 0.0, 32.0)),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old])
        );
        accept_grass_footprints(&mut app);

        // A replicated height correction leaves the XZ exclusions unchanged.
        app.world_mut()
            .entity_mut(building)
            .insert(BuildingPosition(Vec3::new(32.0, 1.0, 32.0)));
        app.update();
        assert!(app
            .world()
            .resource::<ChunkedGroundCoverState>()
            .dirty
            .is_empty());

        app.world_mut()
            .get_mut::<PlacedBuilding>(building)
            .unwrap()
            .rotation = 0.8;
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old])
        );
        accept_grass_footprints(&mut app);
        app.world_mut()
            .entity_mut(building)
            .insert(BuildingPosition(Vec3::new(224.0, 1.0, 32.0)));
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old, new])
        );
        accept_grass_footprints(&mut app);

        // Missing placement components release land even while the entity lives.
        app.world_mut()
            .entity_mut(building)
            .remove::<BuildingPosition>();
        app.update();
        let state = app.world().resource::<ChunkedGroundCoverState>();
        assert_eq!(state.dirty, HashSet::from([new]));
        assert_eq!(state.chunks.len(), 3);
        assert_eq!(state.render_entities.get(&key), Some(&batch));
        assert!(app.world().get_entity(batch).is_ok());
    }

    #[test]
    fn square_metadata_does_not_dirty_grass_but_moving_and_removing_its_land_does() {
        use shared::components::SettlementCivicSquare;
        let mut app = grass_footprint_app();
        let old = ChunkCoord::new(0, 0);
        let new = ChunkCoord::new(3, 0);
        let square = app
            .world_mut()
            .spawn(SettlementCivicSquare {
                center: Vec3::new(32.0, 0.0, 32.0),
                half_extents: Vec2::new(12.0, 8.0),
                rotation: 0.0,
                market_position: Vec3::new(32.0, 0.0, 34.0),
                market_rotation: 0.0,
            })
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old])
        );
        accept_grass_footprints(&mut app);
        app.world_mut()
            .get_mut::<SettlementCivicSquare>(square)
            .unwrap()
            .market_rotation = 0.7;
        app.update();
        assert!(app
            .world()
            .resource::<ChunkedGroundCoverState>()
            .dirty
            .is_empty());
        app.world_mut()
            .get_mut::<SettlementCivicSquare>(square)
            .unwrap()
            .center
            .x = 224.0;
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old, new])
        );
        accept_grass_footprints(&mut app);
        app.world_mut().despawn(square);
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([new])
        );
    }

    #[test]
    fn terrain_mesh_replacement_refreshes_only_resident_grass_and_ignores_material_ticks() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<ChunkedGroundCoverState>()
            .add_systems(Update, mark_chunked_grass_dirty_for_terrain);
        let handles = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            [
                meshes.add(Cuboid::default()),
                meshes.add(Cuboid::new(1.0, 2.0, 1.0)),
            ]
        };
        let coord = ChunkCoord::new(0, 0);
        app.world_mut()
            .resource_mut::<ChunkedGroundCoverState>()
            .chunks
            .insert(
                coord,
                ChunkGrassInstances {
                    terrain_mesh: Some(handles[0].id()),
                    ..default()
                },
            );
        let chunk = app
            .world_mut()
            .spawn((
                TerrainChunk {
                    coord,
                    weightmap: default(),
                    material: default(),
                },
                Mesh3d(handles[0].clone()),
            ))
            .id();
        app.update();
        assert!(app
            .world()
            .resource::<ChunkedGroundCoverState>()
            .dirty
            .is_empty());
        app.world_mut()
            .get_mut::<Mesh3d>(chunk)
            .unwrap()
            .set_changed();
        app.update();
        assert!(app
            .world()
            .resource::<ChunkedGroundCoverState>()
            .dirty
            .is_empty());

        app.world_mut()
            .entity_mut(chunk)
            .insert(Mesh3d(handles[1].clone()));
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([coord])
        );
        {
            let mut state = app.world_mut().resource_mut::<ChunkedGroundCoverState>();
            state.chunks.get_mut(&coord).unwrap().terrain_mesh = Some(handles[1].id());
            state.dirty.clear();
        }
        app.world_mut()
            .get_mut::<Mesh3d>(chunk)
            .unwrap()
            .set_changed();
        app.world_mut().spawn((
            TerrainChunk {
                coord: ChunkCoord::new(8, 0),
                weightmap: default(),
                material: default(),
            },
            Mesh3d(handles[1].clone()),
        ));
        app.update();
        assert!(app
            .world()
            .resource::<ChunkedGroundCoverState>()
            .dirty
            .is_empty());

        // Terrain currently replaces entities, but both lifetime forms work.
        app.world_mut().despawn(chunk);
        app.world_mut().spawn((
            TerrainChunk {
                coord,
                weightmap: default(),
                material: default(),
            },
            Mesh3d(handles[0].clone()),
        ));
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([coord])
        );
    }

    #[test]
    fn tended_meadow_is_continuous_across_chunks_without_a_chunk_repeat() {
        let mut translated_difference = 0.0;
        let mut seed_difference = 0.0;
        for step in -64..=64 {
            let point = Vec2::new(CHUNK_SIZE, step as f32 * 1.7);
            let before = tended_meadow_patch(point - Vec2::X * 0.001, 917);
            let after = tended_meadow_patch(point + Vec2::X * 0.001, 917);
            assert!((before - after).abs() < 0.002);
            let value = tended_meadow_patch(point, 917);
            translated_difference +=
                (value - tended_meadow_patch(point + Vec2::X * CHUNK_SIZE, 917)).abs();
            seed_difference += (value - tended_meadow_patch(point, 918)).abs();
        }
        assert!(
            translated_difference > 10.0,
            "the same pattern must not restart each chunk"
        );
        assert!(
            seed_difference > 10.0,
            "different world seeds need different tending patches"
        );
    }

    #[test]
    fn meadow_tending_keeps_wilderness_and_fuller_tuft_groups() {
        let mut minimum = 1.0_f32;
        let mut maximum = 0.0_f32;
        for x in -16..=16 {
            for z in -16..=16 {
                let point = Vec2::new(x as f32 * 8.0, z as f32 * 8.0);
                let short = tended_grass_retention(point, 917, 1.0, false);
                let tall = tended_grass_retention(point, 917, 1.0, true);
                assert!((0.22..=1.0).contains(&short));
                assert!(tall >= short && tall <= 1.0);
                assert_eq!(
                    tended_grass_retention(point, 917, TENDED_ROAD_VERGE, false),
                    1.0
                );
                assert_eq!(tended_grass_retention(point, 917, f32::INFINITY, true), 1.0);
                assert!(
                    tended_grass_retention(point, 917, TENDED_ROAD_VERGE - 0.001, false) > 0.999
                );
                minimum = minimum.min(short);
                maximum = maximum.max(short);
            }
        }
        assert!(
            minimum < 0.30 && maximum > 0.95,
            "quiet gaps must coexist with full clumps"
        );
    }

    #[test]
    fn moved_and_removed_roads_restore_the_previous_grass_verge() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ChunkedGroundCoverState>()
            .add_systems(Update, mark_chunked_grass_dirty_for_roads);
        let old = ChunkCoord::new(0, 0);
        let new = ChunkCoord::new(3, 0);
        for coord in [old, new] {
            app.world_mut()
                .resource_mut::<ChunkedGroundCoverState>()
                .chunks
                .insert(coord, ChunkGrassInstances::default());
        }
        let entity = app
            .world_mut()
            .spawn(VillageRoad {
                settlement: "Verge test".into(),
                builder: "Builder".into(),
                points: vec![Vec2::new(16., 20.), Vec2::new(32., 20.)],
                built_through: 2,
                width: 2.,
                reserved_width: 4.,
                surface: default(),
                class: default(),
                stone_committed: 0,
            })
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old])
        );
        app.world_mut()
            .resource_mut::<ChunkedGroundCoverState>()
            .dirty
            .clear();
        app.world_mut()
            .get_mut::<VillageRoad>(entity)
            .unwrap()
            .points = vec![Vec2::new(208., 20.), Vec2::new(216., 20.)];
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old, new])
        );
        app.world_mut()
            .resource_mut::<ChunkedGroundCoverState>()
            .dirty
            .clear();
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([new])
        );
    }

    #[test]
    fn clearing_ground_cover_also_discards_previous_world_road_bounds() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.init_resource::<ChunkedGroundCoverState>();
        world.init_resource::<Assets<InstancedGrassMaterial>>();
        let road = world.spawn_empty().id();
        world
            .resource_mut::<ChunkedGroundCoverState>()
            .road_bounds
            .insert(road, (ChunkCoord::new(0, 0), ChunkCoord::new(1, 0)));
        world
            .resource_mut::<ChunkedGroundCoverState>()
            .roads_initialized = true;
        world.despawn(road);
        world.clear_trackers();
        world.clear_trackers();
        world.run_system_once(clear_chunked_ground_cover).unwrap();
        assert!(world
            .resource::<ChunkedGroundCoverState>()
            .road_bounds
            .is_empty());
        assert!(
            !world
                .resource::<ChunkedGroundCoverState>()
                .roads_initialized
        );
    }

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
    fn ground_cover_batches_include_both_fern_variants() {
        let instance = GrassInstance {
            position_height: [0.0, 0.0, 0.0, 1.0],
            rotation_scale: [0.0, 1.0, 1.0, 0.0],
        };
        let data = ChunkGrassInstances {
            fern_a: vec![instance],
            fern_b: vec![instance],
            ..default()
        };
        assert_eq!(data.for_kind(PropKind::FernPatchA).len(), 1);
        assert_eq!(data.for_kind(PropKind::FernPatchB).len(), 1);
        assert_eq!(ground_cover_kinds().len(), 8);
    }

    /// Flower drifts ride the instanced ground-cover path: one buffer per
    /// variant, and every ground-cover kind resolves through the swap-mesh
    /// table `material_for_kind` reads its mesh and material from.
    #[test]
    fn ground_cover_batches_carry_every_flower_variant_through_the_swap_mesh_table() {
        let flowers = [
            PropKind::FlowerA,
            PropKind::FlowerB,
            PropKind::FlowerC,
            PropKind::FlowerD,
        ];
        let mut data = ChunkGrassInstances::default();
        for (index, kind) in flowers.iter().enumerate() {
            let buffer = data.for_kind_mut(*kind).expect("flower buffer");
            buffer.extend(std::iter::repeat_n(
                GrassInstance {
                    position_height: [0.0, 0.0, 0.0, 1.0],
                    rotation_scale: [0.0, 1.0, 1.0, 0.0],
                },
                index + 1,
            ));
        }
        for (index, kind) in flowers.iter().enumerate() {
            assert!(
                ground_cover_kinds().contains(kind),
                "{kind:?} missing from ground_cover_kinds"
            );
            assert_eq!(
                data.for_kind(*kind).len(),
                index + 1,
                "{kind:?} shares a buffer"
            );
        }
        assert_eq!(data.instance_count(), 10);
        for kind in ground_cover_kinds() {
            assert!(
                crate::props::uses_swap_mesh_lod(kind),
                "{kind:?} has no swap-mesh entry, so material_for_kind can never resolve it"
            );
        }
        assert!(ground_cover_kinds().iter().all(|kind| {
            let flower = is_flower_patch(*kind);
            flower == flowers.contains(kind)
        }));
    }

    /// Wild flowers stay out of the tended road verge; the roadside dressing
    /// owns that band. Grass keeps its thinning rule and ferns pass through.
    #[test]
    fn flower_patches_are_kept_off_the_tended_verge() {
        let spawn = |kind: PropKind, x: f32| PropSpawn {
            kind: Some(kind),
            scene_path: String::new(),
            chunk: ChunkCoord { x: 0, z: 0 },
            position: Vec3::new(x, 0.0, 7.5),
            rotation: Quat::IDENTITY,
            scale: 1.0,
            render_tuning: shared::props::PropRenderTuning {
                casts_shadows: false,
                visible_end_distance: None,
            },
        };
        let seed = 91;
        let inside = TENDED_ROAD_VERGE - 1.0;
        let outside = TENDED_ROAD_VERGE + 1.0;
        for kind in [
            PropKind::FlowerA,
            PropKind::FlowerB,
            PropKind::FlowerC,
            PropKind::FlowerD,
        ] {
            assert!(is_flower_patch(kind));
            let mut patch = spawn(kind, 3.0);
            assert!(
                !verge_keeps(&mut patch, inside, seed),
                "{kind:?} inside the tended verge must be dropped"
            );
            let mut patch = spawn(kind, 3.0);
            assert!(
                verge_keeps(&mut patch, outside, seed),
                "{kind:?} beyond the verge is kept"
            );
            assert_eq!(patch.scale, 1.0, "flowers are never shrunk by the verge");
            let mut patch = spawn(kind, 3.0);
            assert!(!verge_keeps(&mut patch, 0.5, seed));
        }
        // Ferns are not verge-tended at all.
        let mut fern = spawn(PropKind::FernPatchA, 3.0);
        assert!(!is_flower_patch(PropKind::FernPatchA));
        assert!(verge_keeps(&mut fern, 1.0, seed));
        assert_eq!(fern.scale, 1.0);
        // Grass follows `tended_grass_retention` inside the verge and is
        // shrunk when kept (0.62 within 3 m of the edge); outside the verge
        // it is untouched.
        assert!(!is_flower_patch(PropKind::GrassShortA));
        let deep = 2.0;
        let mut kept = 0;
        for x in 0..64 {
            let mut tuft = spawn(PropKind::GrassShortA, x as f32 * 0.37);
            let point = tuft.position.xz();
            let retention = tended_grass_retention(point, seed, deep, false);
            let roll = meadow_hash(point.x.to_bits(), point.y.to_bits(), seed ^ 0x7D32_84E9);
            let expected = roll < retention;
            assert_eq!(verge_keeps(&mut tuft, deep, seed), expected);
            if expected {
                kept += 1;
                assert!(
                    (tuft.scale - 0.62).abs() < 1e-6,
                    "kept verge grass is shrunk"
                );
            }
        }
        assert!(
            kept > 0 && kept < 64,
            "verge grass is thinned, not removed or untouched"
        );
        let mut far = spawn(PropKind::GrassShortA, 3.0);
        assert!(verge_keeps(&mut far, outside, seed));
        assert_eq!(far.scale, 1.0);
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

    #[test]
    fn grass_is_abruptly_culled_before_map_view() {
        let range = ground_cover_visibility_range();
        assert!(range.is_abrupt());
        assert!(range.use_aabb);
        assert_eq!(
            range.end_margin,
            GROUND_COVER_END_DISTANCE..GROUND_COVER_END_DISTANCE
        );
    }
    #[test]
    fn port_board_geometry_dirties_only_affected_grass_chunks() {
        use shared::components::{PortGeometry, SettlementId, SettlementPort, ShipKind};
        let mut app = grass_footprint_app();
        let old = ChunkCoord::new(0, 0);
        let new = ChunkCoord::new(3, 0);
        let mut port = SettlementPort {
            settlement: SettlementId(1),
            built: true,
            geometry: PortGeometry {
                shore: Vec3::new(32., 1., 18.),
                pier_end: Vec3::new(32., 1., 46.),
                berth: Vec3::new(37., 0., 46.),
                departure: Vec3::new(37., 0., 58.),
                yaw: 0.,
                maximum_ship: ShipKind::Coaster,
            },
        };
        let entity = app.world_mut().spawn(port).id();
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old])
        );
        accept_grass_footprints(&mut app);
        port.geometry.shore.y += 1.;
        port.geometry.pier_end.y += 1.;
        port.geometry.berth.x += 1.;
        app.world_mut().entity_mut(entity).insert(port);
        app.update();
        assert!(
            app.world()
                .resource::<ChunkedGroundCoverState>()
                .dirty
                .is_empty(),
            "height/berth changes do not change occupied grass ground"
        );
        for point in [
            &mut port.geometry.shore,
            &mut port.geometry.pier_end,
            &mut port.geometry.berth,
            &mut port.geometry.departure,
        ] {
            point.x += 192.;
        }
        app.world_mut().entity_mut(entity).insert(port);
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([old, new])
        );
        accept_grass_footprints(&mut app);
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(
            app.world().resource::<ChunkedGroundCoverState>().dirty,
            HashSet::from([new])
        );
        assert_eq!(
            app.world()
                .resource::<ChunkedGroundCoverState>()
                .chunks
                .len(),
            3,
            "unrelated resident ground is never evicted"
        );
    }
}
