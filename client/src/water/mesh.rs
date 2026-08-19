//! mesh systems.

use super::*;
use shared::water::{WATER_DEPTH_FADE_METERS, WATER_SURFACE_OFFSET};
use std::collections::HashMap;

/// Keep the animated shoreline mesh hidden beneath the bank at its outer edge.
/// This must remain comfortably larger than the maximum shore-lap crest.
const WATER_SHORE_OVERLAP: f32 = 0.18;
/// Horizontal distance (m) at which a vertex counts as fully "open water".
/// Baked into vertex color G so the shader can zone features by distance to
/// the coast — vertical depth alone fails on steep banks, where deep water
/// starts a meter from the shoreline.
const SHORE_DIST_MAX: f32 = 28.0;
/// Cells scanned beyond the chunk when collecting shoreline segments, so
/// distances stay correct across chunk borders (32m at 2m spacing).
const SHORE_SCAN_MARGIN: i32 = 16;
/// Two inexpensive Laplacian passes take the hard 2m-grid corners out of the
/// distance contour used by foam. The actual water cut remains on the terrain
/// crossing, so smoothing cannot uncover dry wedges along a bank.
const SHORE_SMOOTH_PASSES: usize = 2;
/// Never let smoothing pull a bank far enough to change a narrow channel's
/// shape or jump across a terrain cell. This is deliberately below half of
/// the 2m terrain spacing.
const SHORE_SMOOTH_MAX_OFFSET: f32 = VERTEX_SPACING * 0.42;
/// A crossing calculated from either of its two neighbouring cells needs the
/// same identity, including around negative world coordinates. Millimetre
/// quantisation is much finer than f32 terrain precision at the map edge.
const SHORE_KEY_SCALE: f32 = 1024.0;

type ShoreSegment = (Vec2, Vec2);
type ShoreKey = (i32, i32);

#[derive(Clone, Copy)]
struct Corner {
    local_x: f32,
    local_z: f32,
    height: f32,
}

#[derive(Clone, Copy)]
struct WaterVertex {
    pos: [f32; 3],
    /// 1 for ocean, 0 for the river core. Stored in vertex color R so the
    /// ocean shoreline can be calmed without changing the river treatment.
    ocean_factor: f32,
    depth_norm: f32,
    signed_depth_norm: f32,
    shore_dist: f32,
}

/// Crossing of a signed terrain-minus-waterline field along one cell edge.
///
/// Using the signed field, rather than interpolating terrain toward the water
/// level at only the first endpoint, matters for rivers: their surface slopes
/// along the channel, so both the terrain and the local waterline can change
/// across the same two-metre edge.
fn shore_crossing(a: Vec2, a_delta: f32, b: Vec2, b_delta: f32) -> Option<Vec2> {
    if (a_delta < 0.0) == (b_delta < 0.0) {
        return None;
    }
    let denom = a_delta - b_delta;
    let t = if denom.abs() < 1.0e-6 {
        0.5
    } else {
        (a_delta / denom).clamp(0.0, 1.0)
    };
    Some(a.lerp(b, t))
}

fn push_shore_segment(
    segments: &mut Vec<ShoreSegment>,
    edges: &[Option<Vec2>; 4],
    a: usize,
    b: usize,
) {
    if let (Some(a), Some(b)) = (edges[a], edges[b]) {
        segments.push((a, b));
    }
}

/// Connect this marching-squares cell's crossings with the same topology used
/// by the water triangles below. Keeping real line segments instead of an
/// unordered cloud of edge points removes the small Voronoi scallops that used
/// to make the wash and foam masks look blocky along otherwise smooth coasts.
fn append_shore_segments(segments: &mut Vec<ShoreSegment>, mask: u8, edges: [Option<Vec2>; 4]) {
    match mask {
        1 | 14 => push_shore_segment(segments, &edges, 0, 3),
        2 | 13 => push_shore_segment(segments, &edges, 0, 1),
        3 | 12 => push_shore_segment(segments, &edges, 1, 3),
        4 | 11 => push_shore_segment(segments, &edges, 1, 2),
        5 => {
            push_shore_segment(segments, &edges, 0, 3);
            push_shore_segment(segments, &edges, 1, 2);
        }
        6 | 9 => push_shore_segment(segments, &edges, 0, 2),
        7 | 8 => push_shore_segment(segments, &edges, 2, 3),
        10 => {
            push_shore_segment(segments, &edges, 0, 1);
            push_shore_segment(segments, &edges, 2, 3);
        }
        _ => {}
    }
}

fn distance_squared_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let segment = b - a;
    let t = ((p - a).dot(segment) / segment.length_squared().max(1.0e-6)).clamp(0.0, 1.0);
    p.distance_squared(a + segment * t)
}

fn shore_key(point: Vec2) -> ShoreKey {
    (
        (point.x * SHORE_KEY_SCALE).round() as i32,
        (point.y * SHORE_KEY_SCALE).round() as i32,
    )
}

/// Smooth a connected marching-squares contour without increasing its vertex
/// count. The scan margin means every edge used by this chunk has both of its
/// neighbours available, including at chunk boundaries; therefore adjacent
/// chunks calculate byte-for-byte matching seam positions independently.
fn smooth_shore_segments(
    raw_segments: &[ShoreSegment],
) -> (HashMap<ShoreKey, Vec2>, HashMap<ShoreKey, Vec<ShoreKey>>) {
    let mut original = HashMap::<ShoreKey, Vec2>::new();
    let mut neighbours = HashMap::<ShoreKey, Vec<ShoreKey>>::new();

    for &(a, b) in raw_segments {
        let a_key = shore_key(a);
        let b_key = shore_key(b);
        original.entry(a_key).or_insert(a);
        original.entry(b_key).or_insert(b);

        let a_neighbours = neighbours.entry(a_key).or_default();
        if !a_neighbours.contains(&b_key) {
            a_neighbours.push(b_key);
        }
        let b_neighbours = neighbours.entry(b_key).or_default();
        if !b_neighbours.contains(&a_key) {
            b_neighbours.push(a_key);
        }
    }
    for connected in neighbours.values_mut() {
        connected.sort_unstable();
    }

    let mut positions = original.clone();
    for _ in 0..SHORE_SMOOTH_PASSES {
        let mut next = positions.clone();
        for (&key, connected) in &neighbours {
            if connected.len() != 2 {
                continue;
            }
            let Some(&point) = positions.get(&key) else {
                continue;
            };
            let (Some(&previous), Some(&following)) =
                (positions.get(&connected[0]), positions.get(&connected[1]))
            else {
                continue;
            };
            let candidate = point * 0.5 + (previous + following) * 0.25;
            let anchor = original[&key];
            let offset = candidate - anchor;
            next.insert(
                key,
                anchor + offset.clamp_length_max(SHORE_SMOOTH_MAX_OFFSET),
            );
        }
        positions = next;
    }

    (positions, neighbours)
}

/// Four points for one rounded shoreline segment. Endpoints remain shared
/// between cells, while the two interior points follow the connected contour's
/// tangent. The small offset clamp prevents overshoot in tight river bends.
fn shore_curve_points(
    raw_a: Vec2,
    raw_b: Vec2,
    positions: &HashMap<ShoreKey, Vec2>,
    neighbours: &HashMap<ShoreKey, Vec<ShoreKey>>,
) -> [Vec2; 4] {
    const CURVE_MAX_OFFSET: f32 = VERTEX_SPACING * 0.18;

    let a_key = shore_key(raw_a);
    let b_key = shore_key(raw_b);
    let a = positions.get(&a_key).copied().unwrap_or(raw_a);
    let b = positions.get(&b_key).copied().unwrap_or(raw_b);
    let previous = neighbours
        .get(&a_key)
        .and_then(|keys| keys.iter().find(|&&key| key != b_key))
        .and_then(|key| positions.get(key))
        .copied()
        .unwrap_or(a);
    let following = neighbours
        .get(&b_key)
        .and_then(|keys| keys.iter().find(|&&key| key != a_key))
        .and_then(|key| positions.get(key))
        .copied()
        .unwrap_or(b);
    let tangent_a = (b - previous) * 0.5;
    let tangent_b = (following - a) * 0.5;

    let at = |t: f32| {
        let t2 = t * t;
        let t3 = t2 * t;
        let curved = a * (2.0 * t3 - 3.0 * t2 + 1.0)
            + tangent_a * (t3 - 2.0 * t2 + t)
            + b * (-2.0 * t3 + 3.0 * t2)
            + tangent_b * (t3 - t2);
        let linear = a.lerp(b, t);
        linear + (curved - linear).clamp_length_max(CURVE_MAX_OFFSET)
    };

    [a, at(1.0 / 3.0), at(2.0 / 3.0), b]
}

fn add_polygon(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    vertices: &[WaterVertex],
) {
    for i in 1..vertices.len() - 1 {
        add_triangle(
            positions,
            normals,
            colors,
            indices,
            vertices[0],
            vertices[i],
            vertices[i + 1],
        );
    }
}

fn add_triangle(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
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
    // Placeholder values, but the attribute's PRESENCE is load-bearing: it
    // enables toon_water.wgsl's VERTEX_NORMALS path, whose analytic swell
    // normal carries the ocean-shore damping the fragment fallback lacks.
    normals.push([0.0, 1.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    colors.push([
        a.ocean_factor,
        a.shore_dist,
        a.signed_depth_norm,
        a.depth_norm,
    ]);
    colors.push([
        b.ocean_factor,
        b.shore_dist,
        b.signed_depth_norm,
        b.depth_norm,
    ]);
    colors.push([
        c.ocean_factor,
        c.shore_dist,
        c.signed_depth_norm,
        c.depth_norm,
    ]);
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
    let ocean_factor_at = |wx: f32, wz: f32| -> f32 {
        if rivers.is_empty() {
            1.0
        } else {
            1.0 - rivers.weight_at(Vec2::new(wx, wz))
        }
    };
    let waterline_at = |wx: f32, wz: f32| level_at(wx, wz) + WATER_SHORE_OVERLAP;

    // Pass 1: collect connected shoreline segments in and around the chunk so
    // every vertex can carry a continuous horizontal distance to the coast.
    let mut shore_segments: Vec<ShoreSegment> = Vec::new();
    {
        let scan_min = -SHORE_SCAN_MARGIN;
        let scan_max = (CHUNK_RESOLUTION as i32 - 1) + SHORE_SCAN_MARGIN;
        for zi in scan_min..=scan_max {
            for xi in scan_min..=scan_max {
                let x0 = origin_x + xi as f32 * VERTEX_SPACING;
                let z0 = origin_z + zi as f32 * VERTEX_SPACING;
                let x1 = x0 + VERTEX_SPACING;
                let z1 = z0 + VERTEX_SPACING;
                let points = [
                    Vec2::new(x0, z0),
                    Vec2::new(x1, z0),
                    Vec2::new(x1, z1),
                    Vec2::new(x0, z1),
                ];
                let deltas = points.map(|p| terrain.get_height(p.x, p.y) - waterline_at(p.x, p.y));
                let wet = deltas.map(|delta| delta < 0.0);
                let mask = (wet[0] as u8)
                    | ((wet[1] as u8) << 1)
                    | ((wet[2] as u8) << 2)
                    | ((wet[3] as u8) << 3);
                if mask == 0 || mask == 15 {
                    continue;
                }
                let edges = [
                    shore_crossing(points[0], deltas[0], points[1], deltas[1]),
                    shore_crossing(points[1], deltas[1], points[2], deltas[2]),
                    shore_crossing(points[2], deltas[2], points[3], deltas[3]),
                    shore_crossing(points[3], deltas[3], points[0], deltas[0]),
                ];
                append_shore_segments(&mut shore_segments, mask, edges);
            }
        }
    }

    let (smoothed_shore_points, shore_neighbours) = smooth_shore_segments(&shore_segments);
    // Distance queries follow a rounded version of the terrain contour. The
    // rendered cut remains on the exact terrain crossing so smoothing cannot
    // uncover dry triangular wedges along a bank.
    let mut curved_shore_segments = Vec::with_capacity(shore_segments.len() * 3);
    for &(a, b) in &shore_segments {
        let curve = shore_curve_points(a, b, &smoothed_shore_points, &shore_neighbours);
        for pair in curve.windows(2) {
            curved_shore_segments.push((pair[0], pair[1]));
        }
    }
    let shore_segments = curved_shore_segments;

    let shore_dist_norm = |world_x: f32, world_z: f32| -> f32 {
        if shore_segments.is_empty() {
            return 1.0;
        }
        let p = Vec2::new(world_x, world_z);
        let mut best = f32::MAX;
        for &(a, b) in &shore_segments {
            best = best.min(distance_squared_to_segment(p, a, b));
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
            depth_norm: signed_depth_norm.max(0.0),
            signed_depth_norm,
            shore_dist: shore_dist_norm(world_x, world_z),
            ocean_factor: ocean_factor_at(world_x, world_z),
        }
    };

    let edge_crossing = |a: Corner, b: Corner| {
        let a_world = Vec2::new(origin_x + a.local_x, origin_z + a.local_z);
        let b_world = Vec2::new(origin_x + b.local_x, origin_z + b.local_z);
        let a_delta = a.height - waterline_at(a_world.x, a_world.y);
        let b_delta = b.height - waterline_at(b_world.x, b_world.y);
        let denom = a_delta - b_delta;
        let t = if denom.abs() < 1.0e-6 {
            0.5
        } else {
            (a_delta / denom).clamp(0.0, 1.0)
        };
        a_world.lerp(b_world, t)
    };

    // Keep the cut on the exact terrain crossing, but sample its shader data
    // more finely than the 2m source grid.
    let subdivided_boundary = |raw_a: Vec2, raw_b: Vec2| -> [WaterVertex; 4] {
        [
            raw_a,
            raw_a.lerp(raw_b, 1.0 / 3.0),
            raw_a.lerp(raw_b, 2.0 / 3.0),
            raw_b,
        ]
        .map(|world| {
            let local_x = world.x - origin_x;
            let local_z = world.y - origin_z;
            let line = waterline_at(world.x, world.y);
            make_vertex(local_x, local_z, line)
        })
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

            let r0 = if w0 != w1 {
                Some(edge_crossing(c0, c1))
            } else {
                None
            };
            let r1 = if w1 != w2 {
                Some(edge_crossing(c1, c2))
            } else {
                None
            };
            let r2 = if w2 != w3 {
                Some(edge_crossing(c2, c3))
            } else {
                None
            };
            let r3 = if w3 != w0 {
                Some(edge_crossing(c3, c0))
            } else {
                None
            };
            // Boundary cells get two extra samples on their exact crossing.
            // The ocean's visual smoothing happens in the coherent foam field;
            // keeping the cut exact prevents terrain gaps.
            if mask != 15 {
                let mut polygon = |vertices: &[WaterVertex]| {
                    add_polygon(
                        &mut positions,
                        &mut normals,
                        &mut colors,
                        &mut indices,
                        vertices,
                    )
                };
                match mask {
                    1 => {
                        let c = subdivided_boundary(r0.unwrap(), r3.unwrap());
                        polygon(&[v0, c[0], c[1], c[2], c[3]]);
                    }
                    2 => {
                        let c = subdivided_boundary(r1.unwrap(), r0.unwrap());
                        polygon(&[v1, c[0], c[1], c[2], c[3]]);
                    }
                    3 => {
                        let c = subdivided_boundary(r1.unwrap(), r3.unwrap());
                        polygon(&[v0, v1, c[0], c[1], c[2], c[3]]);
                    }
                    4 => {
                        let c = subdivided_boundary(r2.unwrap(), r1.unwrap());
                        polygon(&[v2, c[0], c[1], c[2], c[3]]);
                    }
                    5 => {
                        let a = subdivided_boundary(r0.unwrap(), r3.unwrap());
                        polygon(&[v0, a[0], a[1], a[2], a[3]]);
                        let b = subdivided_boundary(r2.unwrap(), r1.unwrap());
                        polygon(&[v2, b[0], b[1], b[2], b[3]]);
                    }
                    6 => {
                        let c = subdivided_boundary(r2.unwrap(), r0.unwrap());
                        polygon(&[v1, v2, c[0], c[1], c[2], c[3]]);
                    }
                    7 => {
                        let c = subdivided_boundary(r2.unwrap(), r3.unwrap());
                        polygon(&[v0, v1, v2, c[0], c[1], c[2], c[3]]);
                    }
                    8 => {
                        let c = subdivided_boundary(r3.unwrap(), r2.unwrap());
                        polygon(&[v3, c[0], c[1], c[2], c[3]]);
                    }
                    9 => {
                        let c = subdivided_boundary(r0.unwrap(), r2.unwrap());
                        polygon(&[v0, c[0], c[1], c[2], c[3], v3]);
                    }
                    10 => {
                        let a = subdivided_boundary(r1.unwrap(), r0.unwrap());
                        polygon(&[v1, a[0], a[1], a[2], a[3]]);
                        let b = subdivided_boundary(r3.unwrap(), r2.unwrap());
                        polygon(&[v3, b[0], b[1], b[2], b[3]]);
                    }
                    11 => {
                        let c = subdivided_boundary(r1.unwrap(), r2.unwrap());
                        polygon(&[v0, v1, c[0], c[1], c[2], c[3], v3]);
                    }
                    12 => {
                        let c = subdivided_boundary(r3.unwrap(), r1.unwrap());
                        polygon(&[v2, v3, c[0], c[1], c[2], c[3]]);
                    }
                    13 => {
                        let c = subdivided_boundary(r0.unwrap(), r1.unwrap());
                        polygon(&[v0, c[0], c[1], c[2], c[3], v2, v3]);
                    }
                    14 => {
                        let c = subdivided_boundary(r3.unwrap(), r0.unwrap());
                        polygon(&[v1, v2, v3, c[0], c[1], c[2], c[3]]);
                    }
                    _ => {}
                }
                continue;
            }

            add_triangle(
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                v0,
                v1,
                v2,
            );
            add_triangle(
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                v0,
                v2,
                v3,
            );
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
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
    mesh.insert_indices(Indices::U32(indices));

    Some(mesh)
}

/// Maximum distance either side of a centreline a river sets the water level;
/// individual headwater segments taper below it.
/// Beyond this maximum the ocean level takes over, which everywhere inland
/// means "no water". See [`shared::worldgen::RIVER_WATER_REACH`].
use shared::worldgen::{
    river_surface_height, river_water_reach_at, RIVER_WATER_REACH as RIVER_INFLUENCE,
};

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
    /// `(a, b, surface_at_a, surface_at_b, reach_at_a, reach_at_b)` in world XZ.
    segments: Vec<(Vec2, Vec2, f32, f32, f32, f32)>,
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
        let (max_x, max_z) = (
            origin.x + CHUNK_SIZE + margin,
            origin.z + CHUNK_SIZE + margin,
        );

        for river in terrain.rivers() {
            for (segment_index, w) in river.windows(2).enumerate() {
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
                    river_surface_height(a.y, ocean),
                    river_surface_height(b.y, ocean),
                    river_water_reach_at(segment_index, river.len()),
                    river_water_reach_at(segment_index + 1, river.len()),
                ));
            }
        }
        Self { segments }
    }

    fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Smooth 0..1 mask for keeping the established river shader behavior.
    /// The fade occupies only the outer fifth of the river's reach, avoiding
    /// a visible material seam where river water joins the ocean.
    fn weight_at(&self, p: Vec2) -> f32 {
        let mut weight: f32 = 0.0;
        for (a, b, _, _, reach_a, reach_b) in &self.segments {
            let seg = *b - *a;
            let t = ((p - *a).dot(seg) / seg.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let reach = reach_a + (reach_b - reach_a) * t;
            let distance = p.distance(*a + seg * t);
            let edge = ((reach - distance) / (reach * 0.2).max(0.001)).clamp(0.0, 1.0);
            let smooth_edge = edge * edge * (3.0 - 2.0 * edge);
            weight = weight.max(smooth_edge);
        }
        weight
    }

    /// Water level at a world point: the ocean, raised wherever a river runs.
    fn level_at(&self, p: Vec2, ocean: f32) -> f32 {
        let mut level = ocean;
        for (a, b, sa, sb, reach_a, reach_b) in &self.segments {
            let seg = *b - *a;
            let t = ((p - *a).dot(seg) / seg.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let reach = reach_a + (reach_b - reach_a) * t;
            if p.distance_squared(*a + seg * t) < reach * reach {
                level = level.max(sa + (sb - sa) * t);
            }
        }
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_uses_the_full_signed_waterline_field() {
        let crossing =
            shore_crossing(Vec2::new(0.0, 0.0), -0.25, Vec2::new(2.0, 0.0), 0.75).unwrap();

        assert!((crossing.x - 0.5).abs() < 1.0e-6);
        assert_eq!(crossing.y, 0.0);
    }

    #[test]
    fn segment_distance_does_not_scallop_between_edge_samples() {
        let distance = distance_squared_to_segment(
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
        );

        assert!((distance - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn shoreline_segments_follow_water_mesh_topology() {
        let edges = [
            Some(Vec2::new(1.0, 0.0)),
            Some(Vec2::new(2.0, 1.0)),
            Some(Vec2::new(1.0, 2.0)),
            Some(Vec2::new(0.0, 1.0)),
        ];
        let mut segments = Vec::new();

        append_shore_segments(&mut segments, 5, edges);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], (edges[0].unwrap(), edges[3].unwrap()));
        assert_eq!(segments[1], (edges[1].unwrap(), edges[2].unwrap()));
    }

    #[test]
    fn smoothing_removes_grid_zigzags_but_preserves_straight_shores() {
        let zigzag = [
            (Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)),
            (Vec2::new(1.0, 1.0), Vec2::new(2.0, 0.0)),
            (Vec2::new(2.0, 0.0), Vec2::new(3.0, 1.0)),
        ];
        let (smoothed, _) = smooth_shore_segments(&zigzag);
        assert!(smoothed[&shore_key(Vec2::new(1.0, 1.0))].y < 1.0);
        assert!(smoothed[&shore_key(Vec2::new(2.0, 0.0))].y > 0.0);

        let straight = [
            (Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)),
            (Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)),
            (Vec2::new(2.0, 0.0), Vec2::new(3.0, 0.0)),
        ];
        let (smoothed, _) = smooth_shore_segments(&straight);
        assert_eq!(
            smoothed[&shore_key(Vec2::new(1.0, 0.0))],
            Vec2::new(1.0, 0.0)
        );
        assert_eq!(
            smoothed[&shore_key(Vec2::new(2.0, 0.0))],
            Vec2::new(2.0, 0.0)
        );
    }

    #[test]
    fn curved_segments_share_the_smoothed_endpoints() {
        let segments = [
            (Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)),
            (Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0)),
            (Vec2::new(2.0, 1.0), Vec2::new(3.0, 0.0)),
        ];
        let (positions, neighbours) = smooth_shore_segments(&segments);
        let curve = shore_curve_points(segments[1].0, segments[1].1, &positions, &neighbours);

        assert_eq!(curve[0], positions[&shore_key(segments[1].0)]);
        assert_eq!(curve[3], positions[&shore_key(segments[1].1)]);
        assert!(curve[1].is_finite() && curve[2].is_finite());
    }

    #[test]
    fn river_mask_has_a_solid_core_and_smooth_edge() {
        let river = RiverSurface {
            segments: vec![(
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                1.0,
                0.0,
                5.0,
                5.0,
            )],
        };

        assert_eq!(river.weight_at(Vec2::new(5.0, 0.0)), 1.0);
        assert_eq!(river.weight_at(Vec2::new(5.0, 5.0)), 0.0);
        let edge = river.weight_at(Vec2::new(5.0, 4.5));
        assert!(edge > 0.0 && edge < 1.0);
    }
}
