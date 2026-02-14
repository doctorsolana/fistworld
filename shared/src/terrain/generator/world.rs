use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::map::{LoadedMap, MapBounds};
use crate::terrain::TerrainDeltaData;

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
        let span = (terrain.height_max - terrain.height_min).max(0.001);
        let t = ((height - terrain.height_min) / span).clamp(0.0, 1.0);
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

/// Resource holding terrain generator and runtime delta modifications.
#[derive(Resource)]
pub struct WorldTerrain {
    pub generator: TerrainGenerator,
    delta_chunks: HashMap<ChunkCoord, TerrainDeltaData>,
    version: u32,
}

impl Default for WorldTerrain {
    fn default() -> Self {
        Self {
            generator: TerrainGenerator::new(WORLD_SEED),
            delta_chunks: HashMap::new(),
            version: 0,
        }
    }
}

impl WorldTerrain {
    #[inline]
    pub fn get_height(&self, x: f32, z: f32) -> f32 {
        let authored = self.generator.get_height(x, z);
        let delta = self.sample_delta(x, z);
        authored + delta
    }

    #[inline]
    pub fn get_water_height(&self, x: f32, z: f32) -> Option<f32> {
        self.generator.get_water_height(x, z)
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

    /// Apply a flattening rectangle by writing additive deltas over the authored base heightmap.
    pub fn apply_flatten_rect(
        &mut self,
        center: Vec3,
        half_extents: Vec2,
        rotation_y: f32,
        blend_width: f32,
    ) -> Vec<ChunkCoord> {
        let target_height = center.y;
        let cos_r = rotation_y.cos();
        let sin_r = rotation_y.sin();

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
            let world_x = center.x + corner.x * cos_r - corner.y * sin_r;
            let world_z = center.z + corner.x * sin_r + corner.y * cos_r;
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
                        let local_x = rel_x * cos_r + rel_z * sin_r;
                        let local_z = -rel_x * sin_r + rel_z * cos_r;

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
        all_affected
    }

    pub fn modification_version(&self) -> u32 {
        self.version
    }

    pub fn get_delta_chunk(&self, coord: ChunkCoord) -> Option<&TerrainDeltaData> {
        self.delta_chunks.get(&coord)
    }

    pub fn set_delta_chunk(&mut self, coord: ChunkCoord, data: TerrainDeltaData) {
        self.delta_chunks.insert(coord, data);
        self.version = self.version.wrapping_add(1);
    }

    pub fn get_modified_chunk_coords(&self) -> Vec<ChunkCoord> {
        self.delta_chunks.keys().copied().collect()
    }

    pub fn generate_chunk(&self, coord: ChunkCoord) -> ChunkMeshData {
        self.generator
            .generate_chunk_with_deltas(&self.delta_chunks, coord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
}
