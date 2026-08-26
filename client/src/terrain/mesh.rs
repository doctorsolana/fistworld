use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use shared::terrain::{ChunkCoord, ChunkMeshData, TerrainGenerator, CHUNK_RESOLUTION, CHUNK_SIZE};
use shared::worldgen::{
    river_surface_height, river_water_reach_at, RIVER_WATER_REACH as RIVER_INFLUENCE,
};

/// Terrain is normally one vertex every 2m. Only cells that touch a moving
/// waterline are split to 0.5m, removing diagonal shoreline teeth without
/// multiplying the geometry of whole chunks.
const SHORE_TERRAIN_SUBDIVISIONS: usize = 4;
/// Covers the full shore-lap range plus a little room for interpolation.
const SHORE_TERRAIN_REFINE_BAND: f32 = 0.40;

type RiverRenderSegment = (Vec2, Vec2, f32, f32, f32, f32);

struct TerrainMeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tangents: Option<Vec<[f32; 4]>>,
    indices: Vec<u32>,
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn bilerp2(corners: [[f32; 2]; 4], u: f32, v: f32) -> [f32; 2] {
    lerp2(
        lerp2(corners[0], corners[1], u),
        lerp2(corners[2], corners[3], u),
        v,
    )
}

fn bilerp3(corners: [[f32; 3]; 4], u: f32, v: f32) -> [f32; 3] {
    lerp3(
        lerp3(corners[0], corners[1], u),
        lerp3(corners[2], corners[3], u),
        v,
    )
}

fn bilerp4(corners: [[f32; 4]; 4], u: f32, v: f32) -> [f32; 4] {
    lerp4(
        lerp4(corners[0], corners[1], u),
        lerp4(corners[2], corners[3], u),
        v,
    )
}

fn cell_touches_shore_band(signed_heights: [f32; 4]) -> bool {
    let min_height = signed_heights.iter().copied().fold(f32::INFINITY, f32::min);
    let max_height = signed_heights
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    min_height <= SHORE_TERRAIN_REFINE_BAND && max_height >= -SHORE_TERRAIN_REFINE_BAND
}

fn river_segments_for_chunk(
    generator: &TerrainGenerator,
    coord: ChunkCoord,
    ocean: f32,
) -> Vec<RiverRenderSegment> {
    let mut segments = Vec::new();
    let origin = coord.world_pos();
    let margin = RIVER_INFLUENCE;
    let min = Vec2::new(origin.x - margin, origin.z - margin);
    let max = Vec2::new(
        origin.x + CHUNK_SIZE + margin,
        origin.z + CHUNK_SIZE + margin,
    );

    for river in generator.loaded_map().rivers.iter() {
        for (segment_index, window) in river.windows(2).enumerate() {
            let a = Vec2::new(window[0].x, window[0].z);
            let b = Vec2::new(window[1].x, window[1].z);
            if a.x.min(b.x) > max.x
                || a.x.max(b.x) < min.x
                || a.y.min(b.y) > max.y
                || a.y.max(b.y) < min.y
            {
                continue;
            }
            segments.push((
                a,
                b,
                river_surface_height(window[0].y, ocean),
                river_surface_height(window[1].y, ocean),
                river_water_reach_at(segment_index, river.len()),
                river_water_reach_at(segment_index + 1, river.len()),
            ));
        }
    }
    segments
}

fn water_level_at(point: Vec2, ocean: f32, rivers: &[RiverRenderSegment]) -> f32 {
    let mut level = ocean;
    for &(a, b, surface_a, surface_b, reach_a, reach_b) in rivers {
        let segment = b - a;
        let t = ((point - a).dot(segment) / segment.length_squared().max(1.0e-6)).clamp(0.0, 1.0);
        let reach = reach_a + (reach_b - reach_a) * t;
        if point.distance_squared(a + segment * t) < reach * reach {
            level = level.max(surface_a + (surface_b - surface_a) * t);
        }
    }
    level
}

fn shore_refined_buffers(
    mesh_data: &ChunkMeshData,
    tangents: Option<&Vec<[f32; 4]>>,
    generator: &TerrainGenerator,
    coord: ChunkCoord,
) -> TerrainMeshBuffers {
    let Some(ocean) = generator.loaded_map().heightmap.water_level else {
        return TerrainMeshBuffers {
            positions: mesh_data.positions.clone(),
            normals: mesh_data.normals.clone(),
            uvs: mesh_data.uvs.clone(),
            tangents: tangents.cloned(),
            indices: mesh_data.indices.clone(),
        };
    };

    let rivers = river_segments_for_chunk(generator, coord, ocean);
    let origin = coord.world_pos();
    // Compute the signed waterline field once. Most chunks are wholly dry or
    // wholly deep water, so this also lets them retain the original mesh with
    // no subdivision work at all.
    let signed_vertex_heights: Vec<f32> = mesh_data
        .positions
        .iter()
        .map(|position| {
            let world = Vec2::new(origin.x + position[0], origin.z + position[2]);
            position[1] - water_level_at(world, ocean, &rivers)
        })
        .collect();
    let chunk_min = signed_vertex_heights
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let chunk_max = signed_vertex_heights
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    if chunk_min > SHORE_TERRAIN_REFINE_BAND || chunk_max < -SHORE_TERRAIN_REFINE_BAND {
        return TerrainMeshBuffers {
            positions: mesh_data.positions.clone(),
            normals: mesh_data.normals.clone(),
            uvs: mesh_data.uvs.clone(),
            tangents: tangents.cloned(),
            indices: mesh_data.indices.clone(),
        };
    }

    let mut buffers = TerrainMeshBuffers {
        positions: mesh_data.positions.clone(),
        normals: mesh_data.normals.clone(),
        uvs: mesh_data.uvs.clone(),
        tangents: tangents.cloned(),
        indices: Vec::with_capacity(mesh_data.indices.len()),
    };

    for zi in 0..(CHUNK_RESOLUTION - 1) {
        for xi in 0..(CHUNK_RESOLUTION - 1) {
            let i0 = zi * CHUNK_RESOLUTION + xi;
            let i1 = i0 + 1;
            let i2 = i0 + CHUNK_RESOLUTION;
            let i3 = i2 + 1;
            let corner_indices = [i0, i1, i2, i3];
            let positions = corner_indices.map(|index| mesh_data.positions[index]);

            let signed_heights = corner_indices.map(|index| signed_vertex_heights[index]);
            if !cell_touches_shore_band(signed_heights) {
                buffers.indices.extend_from_slice(&[
                    i0 as u32, i2 as u32, i1 as u32, i1 as u32, i2 as u32, i3 as u32,
                ]);
                continue;
            }

            let normals = corner_indices.map(|index| mesh_data.normals[index]);
            let uvs = corner_indices.map(|index| mesh_data.uvs[index]);
            let tangent_corners = tangents.map(|values| corner_indices.map(|index| values[index]));
            let row = SHORE_TERRAIN_SUBDIVISIONS + 1;
            let base = buffers.positions.len() as u32;

            for sub_z in 0..=SHORE_TERRAIN_SUBDIVISIONS {
                let v = sub_z as f32 / SHORE_TERRAIN_SUBDIVISIONS as f32;
                for sub_x in 0..=SHORE_TERRAIN_SUBDIVISIONS {
                    let u = sub_x as f32 / SHORE_TERRAIN_SUBDIVISIONS as f32;
                    buffers.positions.push(bilerp3(positions, u, v));
                    let normal = Vec3::from_array(bilerp3(normals, u, v)).normalize_or_zero();
                    buffers.normals.push(normal.to_array());
                    buffers.uvs.push(bilerp2(uvs, u, v));
                    if let (Some(target), Some(corners)) =
                        (buffers.tangents.as_mut(), tangent_corners)
                    {
                        let tangent = bilerp4(corners, u, v);
                        let direction =
                            Vec3::new(tangent[0], tangent[1], tangent[2]).normalize_or_zero();
                        target.push([
                            direction.x,
                            direction.y,
                            direction.z,
                            if tangent[3] < 0.0 { -1.0 } else { 1.0 },
                        ]);
                    }
                }
            }

            for sub_z in 0..SHORE_TERRAIN_SUBDIVISIONS {
                for sub_x in 0..SHORE_TERRAIN_SUBDIVISIONS {
                    let m0 = base + (sub_z * row + sub_x) as u32;
                    let m1 = m0 + 1;
                    let m2 = m0 + row as u32;
                    let m3 = m2 + 1;
                    buffers.indices.extend_from_slice(&[m0, m2, m1, m1, m2, m3]);
                }
            }
        }
    }

    buffers
}

pub(crate) fn build_terrain_mesh(
    mesh_data: &ChunkMeshData,
    tangents: Option<&Vec<[f32; 4]>>,
    generator: &TerrainGenerator,
    coord: ChunkCoord,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    let buffers = shore_refined_buffers(mesh_data, tangents, generator, coord);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(buffers.positions),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        VertexAttributeValues::Float32x3(buffers.normals),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::Float32x2(buffers.uvs),
    );
    mesh.insert_indices(Indices::U32(buffers.indices));
    if let Some(tangents) = buffers.tangents {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_TANGENT,
            VertexAttributeValues::Float32x4(tangents),
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

/// Record the river-water surface for far-mesh vertices that a river runs through.
///
/// At map scale the far mesh has one vertex every ~16 m and a river is ~13 m
/// wide, so a river usually passes cleanly BETWEEN vertices and colours none of
/// them: zoom out and the rivers vanish. Every paper map ever printed has the
/// same problem and the same answer — draw the river wider than scale. The
/// widening is a rendering decision at map zoom only; the water surface, the
/// carved channel and everything gameplay touches are untouched.
///
/// Rasterised into a flat field rather than tested per vertex: 513x513 vertices
/// against ~250 river segments is 66 million distance tests, and stamping the
/// segments costs a few thousand. Keeping the surface as well as a boolean is
/// essential: the map mesh must put the river at water height and tag it as
/// river water. Inside the moving detail hole the stamp is discarded — the
/// streamed terrain and water render the real channel there.
fn far_river_surfaces(
    terrain: &shared::terrain::WorldTerrain,
    origin: Vec2,
    spacing: f32,
    resolution: usize,
) -> Vec<Option<(f32, f32)>> {
    let mut surfaces: Vec<Option<(f32, f32)>> = vec![None; resolution * resolution];
    let Some(ocean) = terrain.water_level() else {
        return surfaces;
    };
    // 1.5 vertices either side, so a river always lands on a run of vertices
    // and draws as a continuous line rather than a dotted one.
    let radius = spacing * 1.5;
    let cells = (radius / spacing).ceil() as i32;

    for river in terrain.rivers() {
        for window in river.windows(2) {
            let a = Vec2::new(window[0].x, window[0].z);
            let b = Vec2::new(window[1].x, window[1].z);
            let surface_a = river_surface_height(window[0].y, ocean);
            let surface_b = river_surface_height(window[1].y, ocean);
            // Walk the segment finely enough that the stamped discs overlap.
            let steps = ((a.distance(b) / (spacing * 0.5)).ceil() as i32).max(1);
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                let p = a.lerp(b, t);
                let surface = surface_a + (surface_b - surface_a) * t;
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
                            // Stamp only genuine valley floor. Dragging bank
                            // and ridge vertices inside the smear radius down
                            // to the river surface carved an artificial ~50m
                            // canyon into the far mesh: its dark unlit walls
                            // read as an opaque band crossing the whole map at
                            // middle zoom, with pale vertical walls wherever
                            // the bed dropped steeply. Where the river runs in
                            // a gorge the coarse mesh simply shows the gorge.
                            let height = terrain.get_height(world.x, world.y);
                            if height > surface + 2.0 {
                                continue;
                            }
                            // Edge weight 1 on the axis, 0 at the stamp rim:
                            // the colour pass blends partial-weight vertices
                            // toward land so the 16m lattice cannot render
                            // the ribbon as a hard zigzag at map zoom.
                            let weight = 1.0 - world.distance(p) / radius;
                            let target = &mut surfaces[cz as usize * resolution + cx as usize];
                            *target = Some(target.map_or(
                                (surface, weight),
                                |(existing_surface, existing_weight)| {
                                    (existing_surface.max(surface), existing_weight.max(weight))
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
    surfaces
}

pub(crate) fn build_far_terrain_mesh(
    terrain: &shared::terrain::WorldTerrain,
    origin: Vec2,
    spacing: f32,
    resolution: usize,
) -> Mesh {
    let river_surfaces = far_river_surfaces(terrain, origin, spacing, resolution);
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
            let water_level = terrain.generator.loaded_map().heightmap.water_level;
            let is_ocean = matches!(water_level, Some(level) if height <= level);
            // Only the stamp's solid core renders AS the river; rim vertices
            // stay land geometry and just take a colour tint below.
            let river_stamp = river_surfaces[zi * resolution + xi];
            let river_surface = river_stamp
                .filter(|(_, weight)| *weight >= 0.5)
                .map(|(surface, _)| surface);
            let is_water = is_ocean || river_surface.is_some();

            // The far ocean represents the water surface, not the normally-lit
            // seabed. Keep it at one stable height beneath the animated
            // surface's deepest trough; moving it during an LOD transition
            // visibly distorted rivers and shallow coastlines into dark boxes,
            // while placing it inside the swell produced moving gray cutouts.
            let rendered_height = if is_water {
                river_surface.unwrap_or_else(|| water_level.unwrap_or(height))
                    + crate::terrain::map_view::FAR_WATER_SURFACE_OFFSET
            } else {
                height + crate::terrain::map_view::FAR_LAND_Y_OFFSET
            };
            positions.push([local_x, rendered_height, local_z]);
            normals.push(if is_water {
                [0.0, 1.0, 0.0]
            } else {
                [normal.x, normal.y, normal.z]
            });
            uvs.push([world_x / CHUNK_SIZE, world_z / CHUNK_SIZE]);

            // Colour the far mesh the way the close-up terrain reads, so zooming out
            // does not change what the world looks like it is made of.
            //
            // Water matters most here: the real water surface is a per-chunk mesh that
            // only exists near the camera, so without this the ocean renders as land at
            // map scale. Shading it into the terrain itself is what makes a zoomed-out
            // view legible as coastline.
            let palette = crate::terrain::materials::stylized_palette();
            let slope = 1.0 - normal.y.clamp(0.0, 1.0);

            // Rivers first: they sit above sea level, so every branch below
            // would call them land.
            if river_surface.is_some() {
                // The shallow end of the ocean ramp, so a river reads as the
                // same substance as the sea it runs into. Alpha -0.25 is the
                // far material's RIVER marker, deliberately NEGATIVE: coastal
                // vertices carry fractional land coverage in 0..1, and a
                // negative stamp is the only value interpolation against any
                // coverage can never produce. Inside the moving detail hole
                // rivers must be discarded — the detailed terrain and water
                // fully cover them — while the sea-level underlay survives.
                colors.push([0.42, 0.66, 0.78, -0.25]);
                continue;
            }

            let color =
                match water_level {
                    // `<=`: vast areas of ocean floor sit exactly at sea level, and `<` left
                    // them failing into the beach band, painting half the map sand.
                    Some(level) if height <= level => {
                        // Depth shading gives shallows and deep ocean distinct reads, which is
                        // most of what makes a coastline legible from far away.
                        // Match the detailed water shader's depth scale. It reaches
                        // the authored deep color after 2.5m; the former 40m far-
                        // terrain ramp made ordinary ocean look pale right outside
                        // the detailed chunk boundary.
                        let depth = ((level - height) / shared::water::WATER_DEPTH_FADE_METERS)
                            .clamp(0.0, 1.0);
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
                        // Same deliberately soft three-band quantization as
                        // toon_water.wgsl.
                        let depth_banded = depth.lerp((depth * 3.0 + 0.5).floor() / 3.0, 0.35);
                        // The raw shallow material constant is only ever seen up
                        // close through ~70% alpha over a sandy seabed; baked
                        // opaque it rims every coast in neon cyan at map zoom.
                        // Bake the observed composite instead, exactly as the
                        // deep endpoint below bakes its own composite.
                        let shallow = Vec3::from_array([
                            crate::water::WATER_SHALLOW_RGBA[0],
                            crate::water::WATER_SHALLOW_RGBA[1],
                            crate::water::WATER_SHALLOW_RGBA[2],
                        ]) * 0.70
                            + Vec3::new(palette.sand.x, palette.sand.y, palette.sand.z) * 0.30;
                        // The detailed surface is translucent over the seabed, so
                        // its observed deep color is a touch less blue than its
                        // material constant alone. Bake that composite into the
                        // opaque far continuation.
                        let deep = Vec3::new(0.035, 0.105, 0.25);
                        shallow.lerp(deep, depth_banded)
                    }
                    Some(level) if height < level + 1.2 => {
                        Vec3::new(palette.sand.x, palette.sand.y, palette.sand.z)
                    }
                    _ => {
                        // Biome tint so the zoomed-out map reads like the world's
                        // resource layout (matches the minimap's colour language);
                        // legacy maps without a biome field keep the plain grass.
                        // BiomeField expects a gradient-magnitude slope (rise per
                        // metre), not the shader's 1-normal.y measure.
                        let gradient =
                            (normal.x * normal.x + normal.z * normal.z).sqrt() / normal.y.max(0.01);
                        // The SAME smooth blend as the ground weightmap, so the
                        // far mesh and the detail terrain agree about where a
                        // border is and how wide it feathers.
                        let meadow = Vec3::new(palette.grass.x, palette.grass.y, palette.grass.z);
                        let grass =
                            match terrain.generator.loaded_map().biome_field.as_deref().map(
                                |biomes| biomes.biome_blend(world_x, world_z, height, gradient),
                            ) {
                                Some(blend) => {
                                    Vec3::new(0.19, 0.38, 0.17) * blend.forest
                                        + Vec3::new(0.48, 0.42, 0.28) * blend.highlands
                                        + Vec3::new(0.52, 0.50, 0.47) * blend.mountains
                                        + meadow * (blend.meadow() + blend.snow + blend.desert)
                                }
                                None => meadow,
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
                let climate = shared::worldgen::climate_at(seed, world_x, world_z, height, half);
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

            // Alpha is a material marker on this opaque mesh: the far shader
            // shades ocean as unlit water and everything else as ordinary PBR
            // terrain. Near the waterline alpha carries the true sub-lattice
            // LAND COVERAGE (2x2 soft supersample) and RGB blends between the
            // two sides — without this the 16m lattice renders every coast as
            // a hard mid-edge staircase at map zoom.
            let near_coast =
                matches!(water_level, Some(level) if (height - level).abs() < spacing * 0.5);
            let (color, alpha) = match (near_coast, water_level) {
                (true, Some(level)) => {
                    let offset = spacing * 0.25;
                    let mut water_cover = 0.0;
                    for (dx, dz) in [
                        (-offset, -offset),
                        (offset, -offset),
                        (-offset, offset),
                        (offset, offset),
                    ] {
                        let sub = terrain.get_height(world_x + dx, world_z + dz);
                        water_cover += ((level - sub) + 0.5).clamp(0.0, 1.0);
                    }
                    let land_frac = 1.0 - water_cover * 0.25;
                    let sand = Vec3::new(palette.sand.x, palette.sand.y, palette.sand.z);
                    let coast_water = Vec3::from_array([
                        crate::water::WATER_SHALLOW_RGBA[0],
                        crate::water::WATER_SHALLOW_RGBA[1],
                        crate::water::WATER_SHALLOW_RGBA[2],
                    ]) * 0.70
                        + sand * 0.30;
                    if is_water {
                        (color.lerp(sand, land_frac), land_frac)
                    } else {
                        (color.lerp(coast_water, 1.0 - land_frac), land_frac)
                    }
                }
                _ => (color, if is_water { 0.0 } else { 1.0 }),
            };
            // Soften the river ribbon's rim: partial-weight stamp vertices
            // lean toward the river colour while remaining land.
            let color = match river_stamp {
                Some((_, weight)) if weight < 0.5 => {
                    color.lerp(Vec3::new(0.42, 0.66, 0.78), (weight * 1.4).min(0.7))
                }
                _ => color,
            };
            colors.push([color.x, color.y, color.z, alpha]);
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

            if inner_half > 0.0
                && world_x >= inner_min.x
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shoreline_band_refines_crossings_and_moving_lap_margin_only() {
        assert!(cell_touches_shore_band([-0.8, -0.2, 0.2, 0.8]));
        assert!(cell_touches_shore_band([-0.7, -0.5, -0.3, -0.2]));
        assert!(!cell_touches_shore_band([0.41, 0.8, 1.2, 1.6]));
        assert!(!cell_touches_shore_band([-1.6, -1.2, -0.8, -0.41]));
    }

    #[test]
    fn refined_positions_follow_the_heightmaps_bilinear_surface() {
        let corners = [
            [0.0, 0.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 4.0, 2.0],
            [2.0, 8.0, 2.0],
        ];

        assert_eq!(bilerp3(corners, 0.0, 0.0), corners[0]);
        assert_eq!(bilerp3(corners, 1.0, 1.0), corners[3]);
        assert_eq!(bilerp3(corners, 0.5, 0.5), [1.0, 3.5, 1.0]);
    }

    #[test]
    fn far_river_vertices_are_water_at_the_river_surface() {
        let terrain = shared::terrain::WorldTerrain::default();
        let spacing = 16.0;
        let resolution = 5;
        // The valley-floor gate legitimately refuses gorge sections, so probe
        // the start, middle and end of each river until one point stamps.
        let (origin, river_index, river_surface) = terrain
            .rivers()
            .iter()
            .filter(|river| river.len() >= 2)
            .flat_map(|river| {
                [0, river.len() / 2, river.len() - 1]
                    .into_iter()
                    .filter_map(move |i| river.get(i))
            })
            .find_map(|point| {
                let sample = Vec2::new(point.x, point.z);
                let origin = sample - Vec2::splat(spacing * 2.0);
                let surfaces = far_river_surfaces(&terrain, origin, spacing, resolution);
                // Only solid-core stamps (weight >= 0.5) become river geometry.
                let index = surfaces
                    .iter()
                    .position(|entry| entry.is_some_and(|(_, weight)| weight >= 0.5))?;
                Some((origin, index, surfaces[index].unwrap().0))
            })
            .expect("the map-scale river stamp should reach some grid vertex");

        let mesh = build_far_terrain_mesh(&terrain, origin, spacing, resolution);
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(values) => values,
            _ => panic!("unexpected positions"),
        };
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
            VertexAttributeValues::Float32x4(values) => values,
            _ => panic!("unexpected colors"),
        };

        // Alpha -0.25 is the far material's river marker: negative so that
        // interpolation against coastal coverage values can never fake it.
        assert_eq!(colors[river_index][3], -0.25);
        assert!(
            (positions[river_index][1]
                - (river_surface + crate::terrain::map_view::FAR_WATER_SURFACE_OFFSET))
                .abs()
                < 0.001
        );
    }
}
