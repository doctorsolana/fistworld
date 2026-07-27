use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use shared::terrain::{ChunkMeshData, CHUNK_SIZE, MAX_HEIGHT};

pub(crate) fn build_terrain_mesh(
    mesh_data: &ChunkMeshData,
    tangents: Option<&Vec<[f32; 4]>>,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(mesh_data.positions.clone()),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(mesh_data.normals.clone()),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::Float32x2(mesh_data.uvs.clone()),
    );
    mesh.insert_indices(Indices::U32(mesh_data.indices.clone()));
    if let Some(tangents) = tangents {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_TANGENT,
            VertexAttributeValues::Float32x4(tangents.clone()),
        );
    } else {
        let _ = mesh.generate_tangents();
    }
    meshes.add(mesh)
}

pub(crate) fn compute_chunk_tangents(
    mesh_data: &shared::terrain::ChunkMeshData,
) -> Option<Vec<[f32; 4]>> {
    if mesh_data.normals.is_empty() {
        return None;
    }

    // Fast tangent approximation for heightmap terrain:
    // project the +X axis onto the normal plane, and derive handedness
    // so normal maps still read correctly without expensive mikktspace.
    let mut tangents = Vec::with_capacity(mesh_data.normals.len());
    for n in &mesh_data.normals {
        let normal = Vec3::new(n[0], n[1], n[2]).normalize_or_zero();
        let axis = if normal.dot(Vec3::X).abs() > 0.9 {
            Vec3::Z
        } else {
            Vec3::X
        };
        let tangent = (axis - normal * normal.dot(axis)).normalize_or_zero();
        let bitangent = normal.cross(tangent);
        let w = if bitangent.dot(Vec3::Z) < 0.0 {
            -1.0
        } else {
            1.0
        };
        tangents.push([tangent.x, tangent.y, tangent.z, w]);
    }

    Some(tangents)
}

pub(crate) fn build_far_terrain_mesh(
    terrain: &shared::terrain::WorldTerrain,
    origin: Vec2,
    spacing: f32,
    resolution: usize,
) -> Mesh {
    let mut positions = Vec::with_capacity(resolution * resolution);
    let mut normals = Vec::with_capacity(resolution * resolution);
    let mut uvs = Vec::with_capacity(resolution * resolution);
    let mut colors = Vec::with_capacity(resolution * resolution);

    for zi in 0..resolution {
        for xi in 0..resolution {
            let local_x = xi as f32 * spacing;
            let local_z = zi as f32 * spacing;
            let world_x = origin.x + local_x;
            let world_z = origin.y + local_z;

            let height = terrain.get_height(world_x, world_z);
            let normal = terrain.get_normal(world_x, world_z);
            let biome = terrain.get_biome(world_x, world_z);

            positions.push([local_x, height, local_z]);
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([world_x / CHUNK_SIZE, world_z / CHUNK_SIZE]);

            // Colour the far mesh the way the close-up terrain reads, so zooming out
            // does not change what the world looks like it is made of.
            //
            // Water matters most here: the real water surface is a per-chunk mesh that
            // only exists near the camera, so without this the ocean renders as land at
            // map scale. Shading it into the terrain itself is what makes a zoomed-out
            // view legible as coastline.
            let palette = crate::terrain::materials::stylized_palette();
            let water_level = terrain.generator.loaded_map().heightmap.water_level;
            let slope = 1.0 - normal.y.clamp(0.0, 1.0);

            let color = match water_level {
                // `<=`: vast areas of ocean floor sit exactly at sea level, and `<` left
                // them failing into the beach band, painting half the map sand.
                Some(level) if height <= level => {
                    // Depth shading gives shallows and deep ocean distinct reads, which is
                    // most of what makes a coastline legible from far away.
                    let depth = (level - height).clamp(0.0, 40.0) / 40.0;
                    let shallow = Vec3::new(0.52, 0.72, 0.80);
                    let deep = Vec3::new(0.14, 0.26, 0.45);
                    shallow.lerp(deep, depth.powf(0.55))
                }
                Some(level) if height < level + 1.2 => Vec3::new(
                    palette.sand.x,
                    palette.sand.y,
                    palette.sand.z,
                ),
                _ => {
                    // Biome tint so the zoomed-out map reads like the world's
                    // resource layout (matches the minimap's colour language);
                    // legacy maps without a biome field keep the plain grass.
                    // BiomeField expects a gradient-magnitude slope (rise per
                    // metre), not the shader's 1-normal.y measure.
                    let gradient = (normal.x * normal.x + normal.z * normal.z).sqrt()
                        / normal.y.max(0.01);
                    let grass = match terrain
                        .generator
                        .loaded_map()
                        .biome_field
                        .as_deref()
                        .map(|biomes| biomes.biome(world_x, world_z, height, gradient))
                    {
                        Some(shared::worldgen::WorldBiome::Forest) => Vec3::new(0.19, 0.38, 0.17),
                        Some(shared::worldgen::WorldBiome::Highlands) => {
                            Vec3::new(0.48, 0.42, 0.28)
                        }
                        Some(shared::worldgen::WorldBiome::Mountains) => {
                            Vec3::new(0.52, 0.50, 0.47)
                        }
                        _ => Vec3::new(palette.grass.x, palette.grass.y, palette.grass.z),
                    };
                    let rock = Vec3::new(palette.rock.x, palette.rock.y, palette.rock.z);
                    let rockiness = ((slope - 0.30) / 0.32).clamp(0.0, 1.0);
                    let base = grass.lerp(rock, rockiness);
                    // Same height banding as the splat shader so the two agree at the seam.
                    let height_norm = (height / 90.0).clamp(0.0, 1.0);
                    let banded = (height_norm * 5.0).floor() / 5.0;
                    base * (0.94 + banded * 0.16)
                }
            };

            colors.push([color.x, color.y, color.z, 1.0]);
        }
    }

    let indices = build_far_terrain_indices(origin, spacing, Vec2::ZERO, 0.0, resolution);

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
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

pub(crate) fn build_far_terrain_indices(
    origin: Vec2,
    spacing: f32,
    inner_center: Vec2,
    inner_half: f32,
    resolution: usize,
) -> Vec<u32> {
    let mut indices = Vec::with_capacity((resolution - 1) * (resolution - 1) * 6);

    let inner_min = inner_center - Vec2::splat(inner_half);
    let inner_max = inner_center + Vec2::splat(inner_half);

    for z in 0..(resolution - 1) {
        for x in 0..(resolution - 1) {
            let world_x = origin.x + (x as f32 + 0.5) * spacing;
            let world_z = origin.y + (z as f32 + 0.5) * spacing;

            if world_x >= inner_min.x
                && world_x <= inner_max.x
                && world_z >= inner_min.y
                && world_z <= inner_max.y
            {
                continue;
            }

            let i0 = (z * resolution + x) as u32;
            let i1 = (z * resolution + x + 1) as u32;
            let i2 = ((z + 1) * resolution + x) as u32;
            let i3 = ((z + 1) * resolution + x + 1) as u32;

            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    indices
}
