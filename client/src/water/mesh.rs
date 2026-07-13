//! mesh systems.

use super::*;

const WATER_SURFACE_OFFSET: f32 = 0.02;
const WATER_DEPTH_MAX: f32 = 2.5;
const WATER_SHORE_OVERLAP: f32 = 0.12;

#[derive(Clone, Copy)]
struct Corner {
    local_x: f32,
    local_z: f32,
    height: f32,
}

#[derive(Clone, Copy)]
struct WaterVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    depth_norm: f32,
}

fn add_triangle(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    a: WaterVertex,
    b: WaterVertex,
    c: WaterVertex,
) {
    let base = positions.len() as u32;
    positions.push(a.pos);
    positions.push(b.pos);
    positions.push(c.pos);
    normals.push([0.0, 1.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push(a.uv);
    uvs.push(b.uv);
    uvs.push(c.uv);
    colors.push([1.0, 1.0, 1.0, a.depth_norm]);
    colors.push([1.0, 1.0, 1.0, b.depth_norm]);
    colors.push([1.0, 1.0, 1.0, c.depth_norm]);
    // The a/b/c layout below is clockwise seen from above (+Y); emit reversed
    // so the front face points up — otherwise back-face culling hides the
    // whole surface from above water.
    indices.extend_from_slice(&[base, base + 2, base + 1]);
}

pub(super) fn build_water_mesh(terrain: &WorldTerrain, coord: ChunkCoord) -> Option<Mesh> {
    let water_level = terrain.water_level()?;
    let origin = coord.world_pos();
    let origin_x = origin.x;
    let origin_z = origin.z;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let waterline = water_level + WATER_SHORE_OVERLAP;
    let water_y = water_level + WATER_SURFACE_OFFSET;

    let depth_norm =
        |height: f32| ((water_level - height).max(0.0) / WATER_DEPTH_MAX).clamp(0.0, 1.0);

    let make_vertex = |local_x: f32, local_z: f32, depth: f32| {
        let world_x = origin_x + local_x;
        let world_z = origin_z + local_z;
        WaterVertex {
            pos: [local_x, water_y, local_z],
            uv: [world_x / CHUNK_SIZE, world_z / CHUNK_SIZE],
            depth_norm: depth,
        }
    };

    let edge_vertex = |a: Corner, b: Corner| {
        let denom = b.height - a.height;
        let mut t = if denom.abs() < 1e-6 {
            0.5
        } else {
            (waterline - a.height) / denom
        };
        t = t.clamp(0.0, 1.0);
        let local_x = a.local_x + (b.local_x - a.local_x) * t;
        let local_z = a.local_z + (b.local_z - a.local_z) * t;
        make_vertex(local_x, local_z, 0.0)
    };

    for zi in 0..(CHUNK_RESOLUTION - 1) {
        for xi in 0..(CHUNK_RESOLUTION - 1) {
            let x0 = xi as f32 * VERTEX_SPACING;
            let z0 = zi as f32 * VERTEX_SPACING;
            let x1 = (xi + 1) as f32 * VERTEX_SPACING;
            let z1 = (zi + 1) as f32 * VERTEX_SPACING;

            let h0 = terrain.get_height(origin.x + x0, origin.z + z0);
            let h1 = terrain.get_height(origin.x + x1, origin.z + z0);
            let h2 = terrain.get_height(origin.x + x1, origin.z + z1);
            let h3 = terrain.get_height(origin.x + x0, origin.z + z1);

            let c0 = Corner {
                local_x: x0,
                local_z: z0,
                height: h0,
            };
            let c1 = Corner {
                local_x: x1,
                local_z: z0,
                height: h1,
            };
            let c2 = Corner {
                local_x: x1,
                local_z: z1,
                height: h2,
            };
            let c3 = Corner {
                local_x: x0,
                local_z: z1,
                height: h3,
            };

            let w0 = h0 < waterline;
            let w1 = h1 < waterline;
            let w2 = h2 < waterline;
            let w3 = h3 < waterline;

            let mask = (w0 as u8) | ((w1 as u8) << 1) | ((w2 as u8) << 2) | ((w3 as u8) << 3);

            if mask == 0 {
                continue;
            }

            let v0 = make_vertex(c0.local_x, c0.local_z, depth_norm(c0.height));
            let v1 = make_vertex(c1.local_x, c1.local_z, depth_norm(c1.height));
            let v2 = make_vertex(c2.local_x, c2.local_z, depth_norm(c2.height));
            let v3 = make_vertex(c3.local_x, c3.local_z, depth_norm(c3.height));

            let e0 = if w0 != w1 {
                Some(edge_vertex(c0, c1))
            } else {
                None
            };
            let e1 = if w1 != w2 {
                Some(edge_vertex(c1, c2))
            } else {
                None
            };
            let e2 = if w2 != w3 {
                Some(edge_vertex(c2, c3))
            } else {
                None
            };
            let e3 = if w3 != w0 {
                Some(edge_vertex(c3, c0))
            } else {
                None
            };

            match mask {
                1 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v0,
                    e0.unwrap(),
                    e3.unwrap(),
                ),
                2 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v1,
                    e1.unwrap(),
                    e0.unwrap(),
                ),
                3 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        e3.unwrap(),
                    );
                }
                4 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v2,
                    e2.unwrap(),
                    e1.unwrap(),
                ),
                5 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        e2.unwrap(),
                        e1.unwrap(),
                    );
                }
                6 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v2,
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e2.unwrap(),
                        e0.unwrap(),
                    );
                }
                7 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        e3.unwrap(),
                    );
                }
                8 => add_triangle(
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut colors,
                    &mut indices,
                    v3,
                    e3.unwrap(),
                    e2.unwrap(),
                ),
                9 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        v3,
                    );
                }
                10 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e1.unwrap(),
                        e0.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v3,
                        e3.unwrap(),
                        e2.unwrap(),
                    );
                }
                11 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        e2.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e2.unwrap(),
                        v3,
                    );
                }
                12 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        v3,
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v2,
                        e3.unwrap(),
                        e1.unwrap(),
                    );
                }
                13 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e0.unwrap(),
                        e1.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        e1.unwrap(),
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        v3,
                    );
                }
                14 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v2,
                        v3,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        v3,
                        e3.unwrap(),
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v1,
                        e3.unwrap(),
                        e0.unwrap(),
                    );
                }
                15 => {
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v1,
                        v2,
                    );
                    add_triangle(
                        &mut positions,
                        &mut normals,
                        &mut uvs,
                        &mut colors,
                        &mut indices,
                        v0,
                        v2,
                        v3,
                    );
                }
                _ => {}
            }
        }
    }

    if indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
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

    Some(mesh)
}
