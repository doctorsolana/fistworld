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
    // The water level is no longer one number. Rivers raise it along their
    // channels, so every place that used to compare against a constant now asks
    // where it is standing. Chunks with no river nearby short-circuit to the
    // ocean level and cost nothing extra.
    let rivers = RiverSurface::for_chunk(terrain, coord, water_level);
    let level_at = |wx: f32, wz: f32| -> f32 {
        if rivers.is_empty() {
            water_level
        } else {
            rivers.level_at(Vec2::new(wx, wz), water_level)
        }
    };
    let waterline_at = |wx: f32, wz: f32| level_at(wx, wz) + WATER_SHORE_OVERLAP;

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
                let line00 = waterline_at(x0, z0);
                let above00 = h00 >= line00;
                // East edge
                let x1 = x0 + VERTEX_SPACING;
                let h10 = terrain.get_height(x1, z0);
                if above00 != (h10 >= waterline_at(x1, z0)) {
                    let denom = h10 - h00;
                    let t = if denom.abs() < 1e-6 {
                        0.5
                    } else {
                        ((line00 - h00) / denom).clamp(0.0, 1.0)
                    };
                    shore_points.push(Vec2::new(x0 + t * VERTEX_SPACING, z0));
                }
                // South edge
                let z1 = z0 + VERTEX_SPACING;
                let h01 = terrain.get_height(x0, z1);
                if above00 != (h01 >= waterline_at(x0, z1)) {
                    let denom = h01 - h00;
                    let t = if denom.abs() < 1e-6 {
                        0.5
                    } else {
                        ((line00 - h00) / denom).clamp(0.0, 1.0)
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
        let level = level_at(world_x, world_z);
        let signed_depth_norm =
            ((level - terrain_height) / WATER_DEPTH_FADE_METERS).clamp(-1.0, 1.0);
        WaterVertex {
            // Per-vertex height, so a river surface slopes down its valley
            // instead of lying flat like the sea.
            pos: [local_x, level + WATER_SURFACE_OFFSET, local_z],
            uv: [world_x / CHUNK_SIZE, world_z / CHUNK_SIZE],
            depth_norm: signed_depth_norm.max(0.0),
            signed_depth_norm,
            shore_dist: shore_dist_norm(world_x, world_z),
        }
    };

    let edge_vertex = |a: Corner, b: Corner| {
        let line = waterline_at(origin_x + a.local_x, origin_z + a.local_z);
        let denom = b.height - a.height;
        let mut t = if denom.abs() < 1e-6 {
            0.5
        } else {
            (line - a.height) / denom
        };
        t = t.clamp(0.0, 1.0);
        let local_x = a.local_x + (b.local_x - a.local_x) * t;
        let local_z = a.local_z + (b.local_z - a.local_z) * t;
        make_vertex(local_x, local_z, line)
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

            let w0 = h0 < waterline_at(origin.x + x0, origin.z + z0);
            let w1 = h1 < waterline_at(origin.x + x1, origin.z + z0);
            let w2 = h2 < waterline_at(origin.x + x1, origin.z + z1);
            let w3 = h3 < waterline_at(origin.x + x0, origin.z + z1);

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

/// How far either side of a centreline a river still sets the water level.
/// Beyond this the ocean level takes over, which everywhere inland means "no
/// water". Shared with prop scattering, which clears the banks to exactly this
/// reach — see [`shared::worldgen::RIVER_WATER_REACH`].
use shared::worldgen::RIVER_WATER_REACH as RIVER_INFLUENCE;

/// The river water LEVEL near one chunk — not river geometry.
///
/// This used to build its own ribbon of quads along each centreline, and that
/// is the wrong shape of solution. A ribbon is a polyline offset either side by
/// a fixed half-width, so at every bend the outer edges of two neighbouring
/// segments fail to meet and the inner ones overlap: the river was notched all
/// the way down both banks. Mitre joins would fix the notches and still leave a
/// surface whose width had nothing to do with the channel underneath it.
///
/// So rivers do not draw anything of their own any more. They only answer "how
/// high is the water here", and the ocean's existing marching-squares pass —
/// which already walks every cell comparing terrain against water — fills the
/// channel that generation carved. The width becomes the channel's real width,
/// bends are whatever the ground does, the junction with the sea is just two
/// levels agreeing, and there are no seams because there is only one surface.
struct RiverSurface {
    /// `(a, b, surface_at_a, surface_at_b)` in world XZ.
    segments: Vec<(Vec2, Vec2, f32, f32)>,
}

impl RiverSurface {
    fn for_chunk(terrain: &WorldTerrain, coord: ChunkCoord, ocean: f32) -> Self {
        let mut segments = Vec::new();
        let origin = coord.world_pos();
        // The shore scan reaches SHORE_SCAN_MARGIN cells outside the chunk, so
        // the level must be right out there too or the bank distance is wrong
        // at the edges.
        let margin = RIVER_INFLUENCE + SHORE_SCAN_MARGIN as f32 * VERTEX_SPACING;
        let (min_x, min_z) = (origin.x - margin, origin.z - margin);
        let (max_x, max_z) = (origin.x + CHUNK_SIZE + margin, origin.z + CHUNK_SIZE + margin);

        // Surface height for a bed height: full depth inland, tapering to
        // exactly the ocean surface at the mouth, and never below it.
        let surface = |bed: f32| -> f32 {
            let t = ((bed - ocean) / RIVER_MOUTH_BLEND).clamp(0.0, 1.0);
            (bed + RIVER_DEPTH * t).max(ocean)
        };

        for river in terrain.rivers() {
            for w in river.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a.x.min(b.x) > max_x
                    || a.x.max(b.x) < min_x
                    || a.z.min(b.z) > max_z
                    || a.z.max(b.z) < min_z
                {
                    continue;
                }
                segments.push((
                    Vec2::new(a.x, a.z),
                    Vec2::new(b.x, b.z),
                    surface(a.y),
                    surface(b.y),
                ));
            }
        }
        Self { segments }
    }

    fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Water level at a world point: the ocean, raised wherever a river runs.
    fn level_at(&self, p: Vec2, ocean: f32) -> f32 {
        let mut level = ocean;
        for (a, b, sa, sb) in &self.segments {
            let seg = *b - *a;
            let t = ((p - *a).dot(seg) / seg.length_squared().max(1e-6)).clamp(0.0, 1.0);
            if p.distance_squared(*a + seg * t) < RIVER_INFLUENCE * RIVER_INFLUENCE {
                level = level.max(sa + (sb - sa) * t);
            }
        }
        level
    }
}
