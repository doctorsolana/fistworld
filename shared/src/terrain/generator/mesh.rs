use std::collections::HashMap;

use bevy::prelude::*;

use crate::terrain::TerrainDeltaData;

use super::sampling::sample_delta_from_map;
use super::{
    Biome, ChunkCoord, ChunkMeshData, TerrainGenerator, CHUNK_RESOLUTION, CHUNK_SIZE, MAX_HEIGHT,
    VERTEX_SPACING,
};

impl TerrainGenerator {
    /// Generate chunk mesh data using authored map + optional terrain deltas.
    pub fn generate_chunk_with_deltas(
        &self,
        delta_chunks: &HashMap<ChunkCoord, TerrainDeltaData>,
        coord: ChunkCoord,
    ) -> ChunkMeshData {
        let origin = coord.world_pos();
        let vertex_count = CHUNK_RESOLUTION * CHUNK_RESOLUTION;
        let quad_count = (CHUNK_RESOLUTION - 1) * (CHUNK_RESOLUTION - 1);
        let total_index_count = quad_count * 6;
        let mut positions = Vec::with_capacity(vertex_count);
        let mut normals = Vec::with_capacity(vertex_count);
        let mut uvs = Vec::with_capacity(vertex_count);
        let mut colors = Vec::with_capacity(vertex_count);
        let mut material_ids: Vec<u8> = Vec::with_capacity(vertex_count);
        let mut indices: Vec<u32> = Vec::with_capacity(total_index_count);
        let mut grass_indices: Vec<u32> = Vec::with_capacity(total_index_count / 2);
        let mut desert_indices: Vec<u32> = Vec::with_capacity(total_index_count / 2);
        let mut mountain_indices: Vec<u32> = Vec::with_capacity(total_index_count / 2);
        let mut nature_indices: Vec<u32> = Vec::with_capacity(total_index_count / 2);
        let mut base_indices: Vec<u32> = Vec::with_capacity(total_index_count / 2);

        // Build an expanded height stencil once so normals can be sampled without extra terrain calls.
        let stencil_res = CHUNK_RESOLUTION + 2;
        let mut height_stencil = vec![0.0f32; stencil_res * stencil_res];
        let mut authored_height_stencil = vec![0.0f32; stencil_res * stencil_res];
        for sz in 0..stencil_res {
            for sx in 0..stencil_res {
                let world_x = origin.x + (sx as f32 - 1.0) * VERTEX_SPACING;
                let world_z = origin.z + (sz as f32 - 1.0) * VERTEX_SPACING;
                let authored_height = self.get_height(world_x, world_z);
                let delta_height = sample_delta_from_map(delta_chunks, world_x, world_z);
                authored_height_stencil[sz * stencil_res + sx] = authored_height;
                height_stencil[sz * stencil_res + sx] = authored_height + delta_height;
            }
        }

        let rgba_to_arr = |color: Color| -> [f32; 4] {
            let c = color.to_srgba();
            [c.red, c.green, c.blue, c.alpha]
        };
        let lerp_arr = |a: [f32; 4], b: [f32; 4], t: f32| -> [f32; 4] {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
                a[3] + (b[3] - a[3]) * t,
            ]
        };

        let grass_color = rgba_to_arr(Biome::Grasslands.color());
        let nature_color = rgba_to_arr(Biome::Natureland.color());
        let desert_color = rgba_to_arr(Biome::Desert.color());
        let mountain_color = rgba_to_arr(Biome::Mountain.color());
        let ocean_color = rgba_to_arr(Biome::Ocean.color());
        let waterbed_color = rgba_to_arr(Color::srgb(0.18, 0.30, 0.26));
        let authored_water_level = self.loaded_map().heightmap.water_level;
        let center_vertex = CHUNK_RESOLUTION / 2;
        let mut center_biome = Biome::Grasslands;

        for zi in 0..CHUNK_RESOLUTION {
            for xi in 0..CHUNK_RESOLUTION {
                let local_x = xi as f32 * VERTEX_SPACING;
                let local_z = zi as f32 * VERTEX_SPACING;

                let center_idx = (zi + 1) * stencil_res + (xi + 1);
                let height = height_stencil[center_idx];
                let h_left = height_stencil[(zi + 1) * stencil_res + xi];
                let h_right = height_stencil[(zi + 1) * stencil_res + (xi + 2)];
                let h_down = height_stencil[zi * stencil_res + (xi + 1)];
                let h_up = height_stencil[(zi + 2) * stencil_res + (xi + 1)];
                let normal =
                    Vec3::new(h_left - h_right, 2.0 * VERTEX_SPACING, h_down - h_up).normalize();

                let authored_height = authored_height_stencil[center_idx];
                let authored_h_left = authored_height_stencil[(zi + 1) * stencil_res + xi];
                let authored_h_right = authored_height_stencil[(zi + 1) * stencil_res + (xi + 2)];
                let authored_h_down = authored_height_stencil[zi * stencil_res + (xi + 1)];
                let authored_h_up = authored_height_stencil[(zi + 2) * stencil_res + (xi + 1)];
                let authored_normal = Vec3::new(
                    authored_h_left - authored_h_right,
                    2.0 * VERTEX_SPACING,
                    authored_h_down - authored_h_up,
                )
                .normalize();
                let biome = self
                    .biome_from_authored_height_and_normal_y(authored_height, authored_normal.y);

                if xi == center_vertex && zi == center_vertex {
                    center_biome = biome;
                }

                positions.push([local_x, height, local_z]);
                normals.push([normal.x, normal.y, normal.z]);
                uvs.push([local_x / CHUNK_SIZE, local_z / CHUNK_SIZE]);

                let blend = match biome {
                    Biome::Desert | Biome::Ocean => -1.0,
                    Biome::Grasslands => 1.0,
                    Biome::Natureland => 0.8,
                    Biome::Mountain => 0.4,
                };
                let transition_width: f32 = 0.3;
                let t = ((blend / transition_width) * 0.5_f32 + 0.5_f32).clamp(0.0, 1.0);
                let smooth_t = t * t * (3.0 - 2.0 * t);

                let nature_blend = if biome == Biome::Natureland { 1.0 } else { 0.0 };
                let non_desert_color = lerp_arr(grass_color, nature_color, nature_blend);
                let mut base_color = lerp_arr(desert_color, non_desert_color, smooth_t);
                let mountain_mask = if biome == Biome::Mountain { 1.0 } else { 0.0 };
                base_color = lerp_arr(base_color, mountain_color, mountain_mask);

                let mut material_id = match biome {
                    Biome::Grasslands => 0,
                    Biome::Desert => 1,
                    Biome::Mountain => 2,
                    Biome::Natureland => 3,
                    Biome::Ocean => 4,
                };

                if biome == Biome::Ocean {
                    base_color = ocean_color;
                    material_id = 4;
                } else if let Some(water_height) =
                    authored_water_level.filter(|w| *w > authored_height)
                {
                    if water_height > height + 0.05 {
                        base_color = lerp_arr(base_color, waterbed_color, 0.6);
                        material_id = 4;
                    }
                }

                let height_factor = (height / MAX_HEIGHT).clamp(0.0, 1.0);
                let variation = match biome {
                    Biome::Desert => 0.9 + height_factor * 0.15,
                    Biome::Grasslands => 0.95 + height_factor * 0.1,
                    Biome::Natureland => 0.85 + height_factor * 0.2,
                    Biome::Mountain => 0.8 + height_factor * 0.25,
                    Biome::Ocean => 0.85 + height_factor * 0.05,
                };

                colors.push([
                    (base_color[0] * variation).min(1.0),
                    (base_color[1] * variation).min(1.0),
                    (base_color[2] * variation).min(1.0),
                    1.0,
                ]);
                material_ids.push(material_id);
            }
        }

        let mut push_triangle = |i0: u32, i1: u32, i2: u32| {
            indices.extend_from_slice(&[i0, i1, i2]);

            let id0 = material_ids[i0 as usize];
            let id1 = material_ids[i1 as usize];
            let id2 = material_ids[i2 as usize];
            let chosen = if id0 == id1 || id0 == id2 {
                id0
            } else if id1 == id2 {
                id1
            } else {
                id0
            };

            match chosen {
                0 => grass_indices.extend_from_slice(&[i0, i1, i2]),
                1 => desert_indices.extend_from_slice(&[i0, i1, i2]),
                2 => mountain_indices.extend_from_slice(&[i0, i1, i2]),
                3 => nature_indices.extend_from_slice(&[i0, i1, i2]),
                _ => base_indices.extend_from_slice(&[i0, i1, i2]),
            }
        };

        for zi in 0..(CHUNK_RESOLUTION - 1) {
            for xi in 0..(CHUNK_RESOLUTION - 1) {
                let i0 = (zi * CHUNK_RESOLUTION + xi) as u32;
                let i1 = i0 + 1;
                let i2 = i0 + CHUNK_RESOLUTION as u32;
                let i3 = i2 + 1;

                push_triangle(i0, i2, i1);
                push_triangle(i1, i2, i3);
            }
        }

        ChunkMeshData {
            positions,
            normals,
            uvs,
            colors,
            material_ids,
            indices,
            grass_indices,
            desert_indices,
            mountain_indices,
            nature_indices,
            base_indices,
            biome: center_biome,
        }
    }

    pub fn generate_chunk_vertices(&self, coord: ChunkCoord) -> ChunkMeshData {
        self.generate_chunk_with_deltas(&HashMap::new(), coord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_chunk_mesh_has_expected_lengths_and_finite_normals() {
        let generator = TerrainGenerator::new(super::super::WORLD_SEED);
        let mesh = generator.generate_chunk_with_deltas(&HashMap::new(), ChunkCoord::new(0, 0));

        let vertex_count = CHUNK_RESOLUTION * CHUNK_RESOLUTION;
        let index_count = (CHUNK_RESOLUTION - 1) * (CHUNK_RESOLUTION - 1) * 6;

        assert_eq!(mesh.positions.len(), vertex_count);
        assert_eq!(mesh.normals.len(), vertex_count);
        assert_eq!(mesh.uvs.len(), vertex_count);
        assert_eq!(mesh.colors.len(), vertex_count);
        assert_eq!(mesh.material_ids.len(), vertex_count);
        assert_eq!(mesh.indices.len(), index_count);

        for normal in &mesh.normals {
            assert!(normal[0].is_finite());
            assert!(normal[1].is_finite());
            assert!(normal[2].is_finite());
        }

        assert!(mesh.material_ids.iter().all(|id| *id <= 4));

        let split_index_count = mesh.grass_indices.len()
            + mesh.desert_indices.len()
            + mesh.mountain_indices.len()
            + mesh.nature_indices.len()
            + mesh.base_indices.len();
        assert_eq!(split_index_count, mesh.indices.len());
    }
}
