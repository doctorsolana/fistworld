//! mesh systems.

use super::*;
use shared::water::{WATER_DEPTH_FADE_METERS, WATER_SURFACE_OFFSET};

/// Keep the animated shoreline mesh hidden beneath the bank at its outer edge.
/// This must remain comfortably larger than the maximum shore-lap crest.
const WATER_SHORE_OVERLAP: f32 = 0.18;
/// Horizontal distance (m) at which a vertex counts as fully "open water".
/// Baked into vertex color G so the shader can zone features by distance to
/// the coast — vertical depth alone fails on steep banks, where deep water
/// starts a meter from the shoreline.
const SHORE_DIST_MAX: f32 = 28.0;
/// Cells scanned beyond the chunk when collecting shoreline points, so
/// distances stay correct across chunk borders (32m at 2m spacing).
const SHORE_SCAN_MARGIN: i32 = 16;

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
    signed_depth_norm: f32,
    shore_dist: f32,
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
    colors.push([1.0, a.shore_dist, a.signed_depth_norm, a.depth_norm]);
    colors.push([1.0, b.shore_dist, b.signed_depth_norm, b.depth_norm]);
    colors.push([1.0, c.shore_dist, c.signed_depth_norm, c.depth_norm]);
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

    // Pass 1: collect shoreline crossing points in and around the chunk so
    // every vertex can carry its horizontal distance to the coast.
    let mut shore_points: Vec<Vec2> = Vec::new();
    {
        let scan_min = -SHORE_SCAN_MARGIN;
        let scan_max = (CHUNK_RESOLUTION as i32 - 1) + SHORE_SCAN_MARGIN;
        for zi in scan_min..=scan_max {
            for xi in scan_min..=scan_max {
                let x0 = origin_x + xi as f32 * VERTEX_SPACING;
                let z0 = origin_z + zi as f32 * VERTEX_SPACING;
                let h00 = terrain.get_height(x0, z0);
                let above00 = h00 >= waterline;
                // East edge
                let h10 = terrain.get_height(x0 + VERTEX_SPACING, z0);
                if above00 != (h10 >= waterline) {
                    let denom = h10 - h00;
                    let t = if denom.abs() < 1e-6 {
                        0.5
                    } else {
                        ((waterline - h00) / denom).clamp(0.0, 1.0)
                    };
                    shore_points.push(Vec2::new(x0 + t * VERTEX_SPACING, z0));
                }
                // South edge
                let h01 = terrain.get_height(x0, z0 + VERTEX_SPACING);
                if above00 != (h01 >= waterline) {
                    let denom = h01 - h00;
                    let t = if denom.abs() < 1e-6 {
                        0.5
                    } else {
                        ((waterline - h00) / denom).clamp(0.0, 1.0)
                    };
                    shore_points.push(Vec2::new(x0, z0 + t * VERTEX_SPACING));
                }
            }
        }
    }

    let shore_dist_norm = |world_x: f32, world_z: f32| -> f32 {
        if shore_points.is_empty() {
            return 1.0;
        }
        let p = Vec2::new(world_x, world_z);
        let mut best = f32::MAX;
        for point in &shore_points {
            best = best.min(point.distance_squared(p));
        }
        (best.sqrt() / SHORE_DIST_MAX).clamp(0.0, 1.0)
    };

    let make_vertex = |local_x: f32, local_z: f32, terrain_height: f32| {
        let world_x = origin_x + local_x;
        let world_z = origin_z + local_z;
        let signed_depth_norm =
            ((water_level - terrain_height) / WATER_DEPTH_FADE_METERS).clamp(-1.0, 1.0);
        WaterVertex {
            pos: [local_x, water_y, local_z],
            uv: [world_x / CHUNK_SIZE, world_z / CHUNK_SIZE],
            depth_norm: signed_depth_norm.max(0.0),
            signed_depth_norm,
            shore_dist: shore_dist_norm(world_x, world_z),
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
        make_vertex(local_x, local_z, waterline)
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

            let v0 = make_vertex(c0.local_x, c0.local_z, c0.height);
            let v1 = make_vertex(c1.local_x, c1.local_z, c1.height);
            let v2 = make_vertex(c2.local_x, c2.local_z, c2.height);
            let v3 = make_vertex(c3.local_x, c3.local_z, c3.height);

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

    add_river_ribbons(
        terrain,
        coord,
        water_level,
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut colors,
        &mut indices,
    );

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

/// How deep a river runs, in metres, once it is clear of the sea.
///
/// The bed is carved flat, so this is literally how far the surface floats
/// above it. Shallow on purpose: a river reads as a river because you can see
/// the bed through it, and the toon water shader fades toward transparent with
/// depth.
const RIVER_DEPTH: f32 = 0.55;

/// Over how many metres of the final descent the river gives up its depth to
/// meet the sea. Without this the surface arrives at the coast `RIVER_DEPTH`
/// above the ocean and every river mouth ends in a small waterfall.
const RIVER_MOUTH_BLEND: f32 = 6.0;

/// Water half-width. Slightly wider than the flat bed so the edge of the
/// surface tucks under the bank instead of ending in mid-air — the same trick
/// `WATER_SHORE_OVERLAP` plays for the ocean.
const RIVER_WATER_HALF_WIDTH: f32 = shared::worldgen::RIVER_HALF_WIDTH + 1.5;

/// Lay a ribbon of water along each river channel crossing this chunk.
///
/// Rivers are carved into the terrain by generation but the world's water is a
/// single plane at sea level, so before this every river above the waterline —
/// which is nearly all of every river — was a dry ditch. A dry, smooth,
/// even-width, gently-graded channel does not read as a river; it reads as a
/// road, and that is exactly what they were being mistaken for.
///
/// Emitted into the ocean's own per-chunk mesh rather than as a separate
/// system, so rivers inherit streaming, culling, the toon water material and
/// chunk lifetime for free.
#[allow(clippy::too_many_arguments)]
fn add_river_ribbons(
    terrain: &WorldTerrain,
    coord: ChunkCoord,
    water_level: f32,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
) {
    let origin = coord.world_pos();
    let ocean_y = water_level + WATER_SURFACE_OFFSET;

    // A segment belongs to exactly ONE chunk: the one holding its midpoint.
    // Emitting into every chunk the segment touches would draw the same quad
    // twice wherever a river crosses a border, and two coplanar water surfaces
    // z-fight along the seam.
    let owns = |a: Vec3, b: Vec3| -> bool {
        let mid = Vec2::new((a.x + b.x) * 0.5, (a.z + b.z) * 0.5);
        ChunkCoord::from_world_pos(Vec3::new(mid.x, 0.0, mid.y)) == coord
    };

    // Surface height for a bed height: full depth inland, tapering to exactly
    // the ocean surface at the mouth, and never below it.
    let surface = |bed: f32| -> f32 {
        let t = ((bed - water_level) / RIVER_MOUTH_BLEND).clamp(0.0, 1.0);
        (bed + RIVER_DEPTH * t).max(ocean_y)
    };

    for river in terrain.rivers() {
        for window in river.windows(2) {
            let (a, b) = (window[0], window[1]);
            if !owns(a, b) {
                continue;
            }
            let along = Vec2::new(b.x - a.x, b.z - a.z).normalize_or(Vec2::X);
            let side = Vec2::new(-along.y, along.x) * RIVER_WATER_HALF_WIDTH;

            let (ya, yb) = (surface(a.y), surface(b.y));
            // `sign` runs -1..1 across the channel. Depth comes from the actual
            // ground under each corner rather than a constant, so the surface
            // shades deep down the middle and thins to nothing where it meets
            // the bank -- which is also what puts the shader's contact-foam
            // line exactly on the waterline instead of somewhere near it.
            let corner = |p: Vec3, y: f32, sign: f32| -> WaterVertex {
                let world_x = p.x + side.x * sign;
                let world_z = p.z + side.y * sign;
                let ground = terrain.get_height(world_x, world_z);
                let signed = (y - ground) / WATER_DEPTH_FADE_METERS;
                WaterVertex {
                    pos: [world_x - origin.x, y, world_z - origin.z],
                    uv: [world_x / CHUNK_SIZE, world_z / CHUNK_SIZE],
                    depth_norm: signed.clamp(0.0, 1.0),
                    signed_depth_norm: signed.clamp(-1.0, 1.0),
                    // Distance-to-bank, normalised the way the ocean's is, but
                    // measured across the channel because a river's "coast" is
                    // its own two banks and they are metres away, not tens.
                    //
                    // 0.78 in mid-channel is above the shader's shore-foam
                    // cutoff and below its open-water crest threshold, so the
                    // middle of a river is plain moving water. Feeding it 0
                    // everywhere -- "a river is all shore" -- renders the whole
                    // surface as breaking surf: an opaque white ribbon.
                    shore_dist: (1.0 - sign.abs()) * 0.78,
                }
            };

            // THREE vertices across, not two. `shore_dist` and depth both peak
            // mid-channel and fall to zero at the banks, and a quad with only
            // edge vertices interpolates 0 -> 0: the values never reach the
            // middle they describe, and the river renders as two banks with no
            // water between them.
            let (al, ac, ar) = (corner(a, ya, 1.0), corner(a, ya, 0.0), corner(a, ya, -1.0));
            let (bl, bc, br) = (corner(b, yb, 1.0), corner(b, yb, 0.0), corner(b, yb, -1.0));

            // Wound clockwise-seen-from-above, because `add_triangle` reverses
            // whatever it is given (see its comment). Passing the intuitive
            // counter-clockwise order emits downward-facing triangles that
            // back-face culling removes: the geometry is all there, correct,
            // and completely invisible.
            add_triangle(positions, normals, uvs, colors, indices, al, ac, bl);
            add_triangle(positions, normals, uvs, colors, indices, ac, bc, bl);
            add_triangle(positions, normals, uvs, colors, indices, ac, ar, bc);
            add_triangle(positions, normals, uvs, colors, indices, ar, br, bc);
        }
    }
}
