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

/// Mark far-mesh vertices that a river runs through.
///
/// At map scale the far mesh has one vertex every ~16 m and a river is ~13 m
/// wide, so a river usually passes cleanly BETWEEN vertices and colours none of
/// them: zoom out and the rivers vanish. Every paper map ever printed has the
/// same problem and the same answer — draw the river wider than scale. The
/// widening is a rendering decision at map zoom only; the water surface, the
/// carved channel and everything gameplay touches are untouched.
///
/// Rasterised into a flat mask rather than tested per vertex: 513x513 vertices
/// against ~250 river segments is 66 million distance tests, and stamping the
/// segments costs a few thousand.
fn far_river_mask(
    terrain: &shared::terrain::WorldTerrain,
    origin: Vec2,
    spacing: f32,
    resolution: usize,
) -> Vec<bool> {
    let mut mask = vec![false; resolution * resolution];
    // 1.5 vertices either side, so a river always lands on a run of vertices
    // and draws as a continuous line rather than a dotted one.
    let radius = spacing * 1.5;
    let cells = (radius / spacing).ceil() as i32;

    for river in terrain.rivers() {
        for window in river.windows(2) {
            let a = Vec2::new(window[0].x, window[0].z);
            let b = Vec2::new(window[1].x, window[1].z);
            // Walk the segment finely enough that the stamped discs overlap.
            let steps = ((a.distance(b) / (spacing * 0.5)).ceil() as i32).max(1);
            for step in 0..=steps {
                let p = a.lerp(b, step as f32 / steps as f32);
                let gx = ((p.x - origin.x) / spacing).round() as i32;
                let gz = ((p.y - origin.y) / spacing).round() as i32;
                for dz in -cells..=cells {
                    for dx in -cells..=cells {
                        let (cx, cz) = (gx + dx, gz + dz);
                        if cx < 0 || cz < 0 || cx >= resolution as i32 || cz >= resolution as i32 {
                            continue;
                        }
                        let world = Vec2::new(
                            origin.x + cx as f32 * spacing,
                            origin.y + cz as f32 * spacing,
                        );
                        if world.distance(p) <= radius {
                            mask[cz as usize * resolution + cx as usize] = true;
                        }
                    }
                }
            }
        }
    }
    mask
}

pub(crate) fn build_far_terrain_mesh(
    terrain: &shared::terrain::WorldTerrain,
    origin: Vec2,
    spacing: f32,
    resolution: usize,
) -> Mesh {
    let river_mask = far_river_mask(terrain, origin, spacing, resolution);
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

            // Rivers first: they sit above sea level, so every branch below
            // would call them land.
            if river_mask[zi * resolution + xi] {
                // The shallow end of the ocean ramp, so a river reads as the
                // same substance as the sea it runs into.
                colors.push([0.42, 0.66, 0.78, 1.0]);
                continue;
            }

            let color = match water_level {
                // `<=`: vast areas of ocean floor sit exactly at sea level, and `<` left
                // them failing into the beach band, painting half the map sand.
                Some(level) if height <= level => {
                    // Depth shading gives shallows and deep ocean distinct reads, which is
                    // most of what makes a coastline legible from far away.
                    let depth = (level - height).clamp(0.0, 40.0) / 40.0;
                    // Force full deep in the outer rim so the mesh's edge lands
                    // exactly on the infinite-ocean skirt color — otherwise the
                    // map boundary ghosts as a lighter square in the endless sea.
                    let bounds = terrain.generator.active_map_bounds();
                    let dist_to_edge = (world_x - bounds.min[0])
                        .min(bounds.max[0] - world_x)
                        .min(world_z - bounds.min[1])
                        .min(bounds.max[1] - world_z);
                    let rim = 1.0 - (dist_to_edge / 600.0).clamp(0.0, 1.0);
                    let depth = depth.max(rim);
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

            // Climate tint mirrors the splat shader (shared function, so the
            // seam between detail chunks and the far mesh agrees).
            let color = {
                let seed = terrain
                    .generator
                    .loaded_map()
                    .definition
                    .generated
                    .as_ref()
                    .map(|g| g.seed)
                    .unwrap_or(0);
                let bounds = terrain.generator.active_map_bounds();
                let half = (bounds.max[0] - bounds.min[0]) * 0.5;
                let climate =
                    shared::worldgen::climate_at(seed, world_x, world_z, height, half);
                let underwater = matches!(water_level, Some(level) if height <= level);
                if underwater {
                    color
                } else {
                    let frost_tone = color.lerp(Vec3::new(0.62, 0.66, 0.70), 0.55);
                    let mut c = color.lerp(frost_tone, climate.frost * (1.0 - climate.snow));
                    // smoothstep(0.35, 0.65) — must match terrain_splat.wgsl
                    // or the detail/far seam shows a snow step on hillsides.
                    let t = ((slope - 0.35) / 0.30).clamp(0.0, 1.0);
                    let snow_keep = 1.0 - t * t * (3.0 - 2.0 * t);
                    c = c.lerp(Vec3::new(0.87, 0.91, 0.97), climate.snow * snow_keep);
                    // Desert south (mirrors terrain_splat.wgsl): savanna
                    // yellowing, then quadratically toward sand.
                    c = c.lerp(c * Vec3::new(1.14, 1.05, 0.72), climate.dry);
                    c.lerp(
                        Vec3::new(0.82, 0.72, 0.50),
                        climate.dry * climate.dry * 0.55,
                    )
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
