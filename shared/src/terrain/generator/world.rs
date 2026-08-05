use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::map::{LoadedMap, MapBounds};
use crate::terrain::TerrainDeltaData;
use crate::worldgen::{river_surface_height, river_water_reach_at};

use super::map_access::load_active_map;
use super::sampling::sample_delta_from_map;
use super::{
    Biome, ChunkCoord, ChunkMeshData, CHUNK_RESOLUTION, CHUNK_SIZE, VERTEX_SPACING, WORLD_SEED,
};

/// Terrain generator backed by authored map data.
#[derive(Clone)]
pub struct TerrainGenerator {
    loaded_map: Arc<LoadedMap>,
    #[allow(dead_code)]
    seed: u32,
}

impl TerrainGenerator {
    pub fn new(seed: u32) -> Self {
        let loaded_map = load_active_map();
        Self { loaded_map, seed }
    }

    pub fn from_loaded_map(loaded_map: LoadedMap, seed: u32) -> Self {
        Self {
            loaded_map: Arc::new(loaded_map),
            seed,
        }
    }

    #[inline]
    pub fn active_map_id(&self) -> &str {
        &self.loaded_map.definition.map_id
    }

    #[inline]
    pub fn active_map_bounds(&self) -> MapBounds {
        self.loaded_map.definition.bounds
    }

    #[inline]
    pub fn active_map_content_hash(&self) -> u64 {
        self.loaded_map.content_hash
    }

    #[inline]
    pub fn loaded_map(&self) -> &LoadedMap {
        self.loaded_map.as_ref()
    }

    pub fn get_water_height(&self, x: f32, z: f32) -> Option<f32> {
        self.loaded_map.heightmap.get_water_height(x, z)
    }

    pub fn get_height(&self, x: f32, z: f32) -> f32 {
        self.loaded_map.heightmap.sample_height(x, z)
    }

    pub fn get_biome(&self, x: f32, z: f32) -> Biome {
        if !self.loaded_map.heightmap.contains_xz(x, z) {
            return Biome::Ocean;
        }

        let height = self.get_height(x, z);
        let normal_y = self.get_normal(x, z).y;
        self.biome_from_authored_height_and_normal_y(height, normal_y)
    }

    pub fn get_normal(&self, x: f32, z: f32) -> Vec3 {
        let sample_dist = 0.5;
        let h_left = self.get_height(x - sample_dist, z);
        let h_right = self.get_height(x + sample_dist, z);
        let h_back = self.get_height(x, z - sample_dist);
        let h_front = self.get_height(x, z + sample_dist);

        let dx = (h_right - h_left) / (2.0 * sample_dist);
        let dz = (h_front - h_back) / (2.0 * sample_dist);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    /// Returns [grass, dirt, sand, cobblestone].
    pub fn get_surface_weights(&self, x: f32, z: f32) -> [f32; 4] {
        // Generated worlds paint procedurally from the same formula the
        // generator used, so nothing needs baking: beaches from height,
        // rock from slope.
        if self.loaded_map.definition.generated.is_some() {
            let h = self.get_height(x, z);
            let step = super::constants::VERTEX_SPACING;
            let hm = &self.loaded_map.heightmap;
            let dx = (hm.sample_height(x + step, z) - hm.sample_height(x - step, z)) / (2.0 * step);
            let dz = (hm.sample_height(x, z + step) - hm.sample_height(x, z - step)) / (2.0 * step);
            let slope = (dx * dx + dz * dz).sqrt();
            let weights = crate::worldgen::surface_weights_at(h, slope);
            if let Some(biomes) = self.loaded_map.biome_field.as_deref() {
                let biome = biomes.biome(x, z, h, slope);
                return crate::worldgen::biome_adjusted_weights(weights, biome);
            }
            return weights;
        }

        match self.get_biome(x, z) {
            Biome::Desert => [0.02, 0.05, 0.93, 0.0],
            Biome::Grasslands => [0.85, 0.10, 0.05, 0.0],
            Biome::Natureland => [0.10, 0.82, 0.08, 0.0],
            Biome::Mountain => [0.05, 0.70, 0.10, 0.15],
            Biome::Ocean => [0.0, 0.05, 0.95, 0.0],
        }
    }

    #[inline]
    pub(crate) fn biome_from_authored_height_and_normal_y(
        &self,
        height: f32,
        normal_y: f32,
    ) -> Biome {
        if let Some(water) = self.loaded_map.heightmap.water_level {
            if height < water {
                return Biome::Ocean;
            }
        }

        let terrain = &self.loaded_map.definition.terrain;
        let raw_span = terrain.height_max - terrain.height_min;
        let t = if raw_span.abs() <= 0.001 {
            0.5
        } else {
            ((height - terrain.height_min) / raw_span).clamp(0.0, 1.0)
        };
        let slope = 1.0 - normal_y.clamp(0.0, 1.0);

        if slope > 0.35 || t > 0.85 {
            Biome::Mountain
        } else if slope > 0.18 {
            Biome::Natureland
        } else if t < 0.30 {
            Biome::Desert
        } else {
            Biome::Grasslands
        }
    }
}

/// One rendered river-water segment in ground-plane coordinates.
#[derive(Clone, Copy, Debug)]
struct RiverWaterSegment {
    a: Vec2,
    b: Vec2,
    surface_a: f32,
    surface_b: f32,
    reach_a: f32,
    reach_b: f32,
}

impl RiverWaterSegment {
    fn level_at(self, point: Vec2) -> Option<f32> {
        let segment = self.b - self.a;
        let t =
            ((point - self.a).dot(segment) / segment.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let reach = self.reach_a + (self.reach_b - self.reach_a) * t;
        (point.distance_squared(self.a + segment * t) < reach * reach)
            .then_some(self.surface_a + (self.surface_b - self.surface_a) * t)
    }
}

/// Chunked lookup for local river height.
///
/// Road A* asks thousands of water questions. Indexing each short river
/// segment into the few terrain chunks touched by its tapered reach keeps each
/// query to a handful of segments rather than every river in the world.
#[derive(Default)]
struct RiverWaterIndex {
    by_chunk: HashMap<(i32, i32), Vec<RiverWaterSegment>>,
}

impl RiverWaterIndex {
    fn from_loaded_map(map: &LoadedMap) -> Self {
        let Some(ocean) = map.heightmap.water_level else {
            return Self::default();
        };
        let mut index = Self::default();
        for river in map.rivers.iter() {
            for (segment_index, window) in river.windows(2).enumerate() {
                let a3 = window[0];
                let b3 = window[1];
                let segment = RiverWaterSegment {
                    a: Vec2::new(a3.x, a3.z),
                    b: Vec2::new(b3.x, b3.z),
                    surface_a: river_surface_height(a3.y, ocean),
                    surface_b: river_surface_height(b3.y, ocean),
                    reach_a: river_water_reach_at(segment_index, river.len()),
                    reach_b: river_water_reach_at(segment_index + 1, river.len()),
                };
                let reach = segment.reach_a.max(segment.reach_b);
                let min = segment.a.min(segment.b) - Vec2::splat(reach);
                let max = segment.a.max(segment.b) + Vec2::splat(reach);
                let min_chunk = (
                    (min.x / CHUNK_SIZE).floor() as i32,
                    (min.y / CHUNK_SIZE).floor() as i32,
                );
                let max_chunk = (
                    (max.x / CHUNK_SIZE).floor() as i32,
                    (max.y / CHUNK_SIZE).floor() as i32,
                );
                for x in min_chunk.0..=max_chunk.0 {
                    for z in min_chunk.1..=max_chunk.1 {
                        index.by_chunk.entry((x, z)).or_default().push(segment);
                    }
                }
            }
        }
        index
    }

    fn level_at(&self, point: Vec2, ocean: f32) -> f32 {
        let key = (
            (point.x / CHUNK_SIZE).floor() as i32,
            (point.y / CHUNK_SIZE).floor() as i32,
        );
        self.by_chunk
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|segment| segment.level_at(point))
            .fold(ocean, f32::max)
    }
}

/// Resource holding terrain generator and runtime delta modifications.
#[derive(Resource)]
pub struct WorldTerrain {
    pub generator: TerrainGenerator,
    river_water: RiverWaterIndex,
    delta_chunks: HashMap<ChunkCoord, TerrainDeltaData>,
    version: u32,
    chunk_versions: HashMap<ChunkCoord, u32>,
    full_rebuild_version: u32,
}

impl Default for WorldTerrain {
    fn default() -> Self {
        let generator = TerrainGenerator::new(WORLD_SEED);
        let river_water = RiverWaterIndex::from_loaded_map(generator.loaded_map());
        let delta_chunks = generator.loaded_map().terrain_deltas_by_chunk.clone();

        Self {
            generator,
            river_water,
            delta_chunks,
            version: 0,
            chunk_versions: HashMap::new(),
            full_rebuild_version: 0,
        }
    }
}

impl WorldTerrain {
    pub fn reload_from_loaded_map(&mut self, loaded_map: LoadedMap) {
        super::map_access::set_active_map_bounds(loaded_map.definition.bounds);
        self.river_water = RiverWaterIndex::from_loaded_map(&loaded_map);
        let delta_chunks = loaded_map.terrain_deltas_by_chunk.clone();
        self.generator = TerrainGenerator::from_loaded_map(loaded_map, WORLD_SEED);
        self.delta_chunks = delta_chunks;
        self.version = self.version.wrapping_add(1);
        self.chunk_versions.clear();
        self.full_rebuild_version = self.full_rebuild_version.wrapping_add(1);
    }

    #[inline]
    pub fn water_level(&self) -> Option<f32> {
        self.generator.loaded_map().heightmap.water_level
    }

    /// Water surface at a world point, including sloping inland rivers.
    ///
    /// Ocean-only callers may still use [`Self::water_level`]. Placement,
    /// navigation and buoyancy need this local surface instead: an inland
    /// river can be far above the global sea plane.
    #[inline]
    pub fn water_surface_height(&self, x: f32, z: f32) -> Option<f32> {
        let ocean = self.water_level()?;
        Some(self.river_water.level_at(Vec2::new(x, z), ocean))
    }

    /// River centrelines as `(x, bed_height, z)`. Empty for authored maps.
    #[inline]
    pub fn rivers(&self) -> &[Vec<Vec3>] {
        &self.generator.loaded_map().rivers
    }

    #[inline]
    pub fn get_height(&self, x: f32, z: f32) -> f32 {
        let authored = self.generator.get_height(x, z);
        let delta = self.sample_delta(x, z);
        authored + delta
    }

    #[inline]
    pub fn get_water_height(&self, x: f32, z: f32) -> Option<f32> {
        let water = self.water_surface_height(x, z)?;
        (self.get_height(x, z) < water).then_some(water)
    }

    fn sample_delta(&self, x: f32, z: f32) -> f32 {
        sample_delta_from_map(&self.delta_chunks, x, z)
    }

    pub fn get_normal(&self, x: f32, z: f32) -> Vec3 {
        let sample_dist = 0.5;

        let h_left = self.get_height(x - sample_dist, z);
        let h_right = self.get_height(x + sample_dist, z);
        let h_back = self.get_height(x, z - sample_dist);
        let h_front = self.get_height(x, z + sample_dist);

        let dx = (h_right - h_left) / (2.0 * sample_dist);
        let dz = (h_front - h_back) / (2.0 * sample_dist);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    #[inline]
    pub fn get_biome(&self, x: f32, z: f32) -> Biome {
        self.generator.get_biome(x, z)
    }

    pub fn apply_additive_circle(
        &mut self,
        center_xz: Vec2,
        radius: f32,
        amount: f32,
    ) -> Vec<ChunkCoord> {
        if radius <= 0.0 || amount.abs() <= f32::EPSILON {
            return Vec::new();
        }

        let min_chunk_x = ((center_xz.x - radius) / CHUNK_SIZE).floor() as i32;
        let max_chunk_x = ((center_xz.x + radius) / CHUNK_SIZE).floor() as i32;
        let min_chunk_z = ((center_xz.y - radius) / CHUNK_SIZE).floor() as i32;
        let max_chunk_z = ((center_xz.y + radius) / CHUNK_SIZE).floor() as i32;

        let mut affected_chunks = Vec::new();
        let radius_sq = radius * radius;

        for chunk_x in min_chunk_x..=max_chunk_x {
            for chunk_z in min_chunk_z..=max_chunk_z {
                let coord = ChunkCoord::new(chunk_x, chunk_z);
                let origin = coord.world_pos();
                let mut modified = false;
                let delta_data = self.delta_chunks.entry(coord).or_default();

                for zi in 0..CHUNK_RESOLUTION {
                    for xi in 0..CHUNK_RESOLUTION {
                        let world_x = origin.x + xi as f32 * VERTEX_SPACING;
                        let world_z = origin.z + zi as f32 * VERTEX_SPACING;

                        let dx = world_x - center_xz.x;
                        let dz = world_z - center_xz.y;
                        let dist_sq = dx * dx + dz * dz;
                        if dist_sq > radius_sq {
                            continue;
                        }

                        let dist = dist_sq.sqrt();
                        let t = (1.0 - dist / radius).clamp(0.0, 1.0);
                        let falloff = t * t * (3.0 - 2.0 * t);
                        delta_data.add_vertex(xi, zi, amount * falloff);
                        modified = true;
                    }
                }

                if modified {
                    delta_data.version = delta_data.version.wrapping_add(1);
                    affected_chunks.push(coord);
                }
            }
        }

        if affected_chunks.is_empty() {
            return affected_chunks;
        }

        self.version = self.version.wrapping_add(1);

        let mut all_affected: HashSet<ChunkCoord> = affected_chunks.iter().copied().collect();
        for chunk in &affected_chunks {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    all_affected.insert(ChunkCoord::new(chunk.x + dx, chunk.z + dz));
                }
            }
        }

        let mut all_affected: Vec<ChunkCoord> = all_affected.into_iter().collect();
        all_affected.sort_by_key(|coord| (coord.x, coord.z));
        self.mark_chunks_modified(all_affected.iter().copied());
        all_affected
    }

    /// Apply a flattening rectangle by writing additive deltas over the authored base heightmap.
    pub fn apply_flatten_rect(
        &mut self,
        center: Vec3,
        half_extents: Vec2,
        rotation_y: f32,
        blend_width: f32,
    ) -> Vec<ChunkCoord> {
        let target_height = center.y;
        let corners = [
            Vec2::new(-half_extents.x - blend_width, -half_extents.y - blend_width),
            Vec2::new(half_extents.x + blend_width, -half_extents.y - blend_width),
            Vec2::new(-half_extents.x - blend_width, half_extents.y + blend_width),
            Vec2::new(half_extents.x + blend_width, half_extents.y + blend_width),
        ];

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;

        for corner in &corners {
            // Same convention as the model and the build zone. This used to
            // rotate the opposite way -- internally consistent, so the levelled
            // patch was the right SHAPE, just turned the wrong way relative to
            // the building standing on it.
            let world = crate::rotation::local_to_world_xz(*corner, rotation_y);
            let world_x = center.x + world.x;
            let world_z = center.z + world.y;
            min_x = min_x.min(world_x);
            max_x = max_x.max(world_x);
            min_z = min_z.min(world_z);
            max_z = max_z.max(world_z);
        }

        let min_chunk_x = (min_x / CHUNK_SIZE).floor() as i32;
        let max_chunk_x = (max_x / CHUNK_SIZE).floor() as i32;
        let min_chunk_z = (min_z / CHUNK_SIZE).floor() as i32;
        let max_chunk_z = (max_z / CHUNK_SIZE).floor() as i32;

        let mut affected_chunks = Vec::new();

        for chunk_x in min_chunk_x..=max_chunk_x {
            for chunk_z in min_chunk_z..=max_chunk_z {
                let chunk_coord = ChunkCoord::new(chunk_x, chunk_z);
                let chunk_origin = chunk_coord.world_pos();
                let mut chunk_modified = false;

                let delta_data = self.delta_chunks.entry(chunk_coord).or_default();

                for zi in 0..CHUNK_RESOLUTION {
                    for xi in 0..CHUNK_RESOLUTION {
                        let world_x = chunk_origin.x + xi as f32 * VERTEX_SPACING;
                        let world_z = chunk_origin.z + zi as f32 * VERTEX_SPACING;

                        let rel_x = world_x - center.x;
                        let rel_z = world_z - center.z;
                        let local =
                            crate::rotation::world_to_local_xz(Vec2::new(rel_x, rel_z), rotation_y);
                        let (local_x, local_z) = (local.x, local.y);

                        let dist_x = local_x.abs() - half_extents.x;
                        let dist_z = local_z.abs() - half_extents.y;

                        let blend_factor = if dist_x <= 0.0 && dist_z <= 0.0 {
                            1.0
                        } else if dist_x <= blend_width && dist_z <= blend_width {
                            let edge_dist = dist_x.max(0.0).max(dist_z.max(0.0));
                            if edge_dist >= blend_width {
                                0.0
                            } else {
                                let t = edge_dist / blend_width;
                                1.0 - t * t * (3.0 - 2.0 * t)
                            }
                        } else {
                            0.0
                        };

                        if blend_factor > 0.0 {
                            chunk_modified = true;

                            let authored_h = self.generator.get_height(world_x, world_z);
                            let current_delta = delta_data.get_vertex(xi, zi);
                            let current_h = authored_h + current_delta;
                            let desired_h = target_height;

                            let height_change = (desired_h - current_h) * blend_factor;
                            delta_data.add_vertex(xi, zi, height_change);
                        }
                    }
                }

                if chunk_modified {
                    delta_data.version = delta_data.version.wrapping_add(1);
                    affected_chunks.push(chunk_coord);
                }
            }
        }

        self.version = self.version.wrapping_add(1);

        let mut all_affected: HashSet<ChunkCoord> = affected_chunks.iter().copied().collect();
        for chunk in &affected_chunks {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx != 0 || dz != 0 {
                        all_affected.insert(ChunkCoord::new(chunk.x + dx, chunk.z + dz));
                    }
                }
            }
        }

        let mut all_affected: Vec<ChunkCoord> = all_affected.into_iter().collect();
        all_affected.sort_by_key(|c| (c.x, c.z));
        self.mark_chunks_modified(all_affected.iter().copied());
        all_affected
    }

    pub fn modification_version(&self) -> u32 {
        self.version
    }

    /// Version for changes that require every derived terrain chunk to be rebuilt.
    pub fn full_rebuild_version(&self) -> u32 {
        self.full_rebuild_version
    }

    /// Version of the height data affecting one terrain chunk.
    pub fn chunk_modification_version(&self, coord: ChunkCoord) -> u32 {
        self.chunk_versions.get(&coord).copied().unwrap_or(0)
    }

    pub fn get_delta_chunk(&self, coord: ChunkCoord) -> Option<&TerrainDeltaData> {
        self.delta_chunks.get(&coord)
    }

    pub fn set_delta_chunk(&mut self, coord: ChunkCoord, data: TerrainDeltaData) {
        self.delta_chunks.insert(coord, data);
        self.version = self.version.wrapping_add(1);
        self.mark_chunks_modified(
            ((-1)..=1).flat_map(|dx| {
                ((-1)..=1).map(move |dz| ChunkCoord::new(coord.x + dx, coord.z + dz))
            }),
        );
    }

    pub fn replace_delta_chunks(&mut self, chunks: HashMap<ChunkCoord, TerrainDeltaData>) {
        self.delta_chunks = chunks;
        self.version = self.version.wrapping_add(1);
        self.chunk_versions.clear();
        self.full_rebuild_version = self.full_rebuild_version.wrapping_add(1);
    }

    pub fn get_modified_chunk_coords(&self) -> Vec<ChunkCoord> {
        self.delta_chunks.keys().copied().collect()
    }

    pub fn delta_chunks(&self) -> &HashMap<ChunkCoord, TerrainDeltaData> {
        &self.delta_chunks
    }

    pub fn generate_chunk(&self, coord: ChunkCoord) -> ChunkMeshData {
        self.generator
            .generate_chunk_with_deltas(&self.delta_chunks, coord)
    }

    fn mark_chunks_modified(&mut self, coords: impl IntoIterator<Item = ChunkCoord>) {
        for coord in coords {
            if !coord.in_world_bounds() {
                continue;
            }
            let version = self.chunk_versions.entry(coord).or_default();
            *version = version.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn river_segment_reports_its_sloping_local_surface_only_inside_its_reach() {
        let segment = RiverWaterSegment {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(10.0, 0.0),
            surface_a: 12.0,
            surface_b: 8.0,
            reach_a: 2.0,
            reach_b: 4.0,
        };

        assert_eq!(segment.level_at(Vec2::new(5.0, 1.0)), Some(10.0));
        assert_eq!(segment.level_at(Vec2::new(5.0, 4.0)), None);
    }

    #[test]
    fn world_water_query_detects_an_inland_river_above_the_ocean_plane() {
        let terrain = WorldTerrain::default();
        let ocean = terrain.water_level().expect("generated world has water");
        let sample = terrain
            .rivers()
            .iter()
            .flatten()
            .find_map(|point| {
                let surface = terrain.water_surface_height(point.x, point.z)?;
                (surface > ocean + 0.2 && terrain.get_height(point.x, point.z) < surface)
                    .then_some((*point, surface))
            })
            .expect("generated world has a wet inland river point");

        assert_eq!(
            terrain.get_water_height(sample.0.x, sample.0.z),
            Some(sample.1)
        );
    }

    #[test]
    fn flatten_rect_returns_sorted_unique_affected_chunks() {
        let mut terrain = WorldTerrain::default();
        let affected = terrain.apply_flatten_rect(
            Vec3::new(0.0, 2.0, 0.0),
            Vec2::new(CHUNK_SIZE, CHUNK_SIZE),
            0.0,
            2.0,
        );

        let unique_len = affected.iter().copied().collect::<HashSet<_>>().len();
        assert_eq!(unique_len, affected.len());
        assert!(!affected.is_empty());

        for pair in affected.windows(2) {
            let a = pair[0];
            let b = pair[1];
            assert!((a.x, a.z) <= (b.x, b.z));
        }
    }

    #[test]
    fn delta_update_advances_only_nearby_chunk_versions() {
        let mut terrain = WorldTerrain::default();
        let edited = ChunkCoord::new(2, 3);
        let nearby = ChunkCoord::new(3, 4);
        let distant = ChunkCoord::new(12, 12);

        let nearby_before = terrain.chunk_modification_version(nearby);
        let distant_before = terrain.chunk_modification_version(distant);
        terrain.set_delta_chunk(edited, TerrainDeltaData::default());

        assert!(terrain.chunk_modification_version(edited) > 0);
        assert!(terrain.chunk_modification_version(nearby) > nearby_before);
        assert_eq!(terrain.chunk_modification_version(distant), distant_before);
    }

    #[test]
    fn replacing_all_deltas_requests_a_full_rebuild() {
        let mut terrain = WorldTerrain::default();
        let full_rebuild_before = terrain.full_rebuild_version();
        let mut chunks = std::collections::HashMap::new();
        chunks.insert(ChunkCoord::new(1, 1), TerrainDeltaData::default());

        terrain.replace_delta_chunks(chunks);

        assert_ne!(terrain.full_rebuild_version(), full_rebuild_before);
    }
}
