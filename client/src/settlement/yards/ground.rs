//! Cached rendered terrain contact for small household soil patches and props.
//!
//! Each patch is clipped to the real terrain triangles, including the existing
//! shore refinement. A rotated bed therefore cannot bridge a diagonal and let
//! the meadow cut through it. The cache is built once for both yard LODs.

use bevy::prelude::*;
use shared::components::HouseholdYard;
use shared::rotation::{local_to_world_xz, world_to_local_xz};
use shared::terrain::{WorldTerrain, VERTEX_SPACING};

use super::mesh::YardMesh;

const SOIL_LIFT: f32 = 0.025;

pub(super) struct Ground {
    origin: Vec2,
    yaw: f32,
    minimum: Vec2,
    columns: usize,
    rows: usize,
    heights: Vec<f32>,
    subdivisions: Vec<usize>,
}

impl Ground {
    pub(super) fn new(
        yard: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
        terrain: &WorldTerrain,
    ) -> Self {
        let (lo, hi) = yard.world_bounds(origin, yaw, 1.0);
        let mut result = Self::sample(lo, hi, origin.xz(), yaw, |p| terrain.get_height(p.x, p.y));
        if terrain.water_level().is_some() {
            for z in 0..result.rows - 1 {
                for x in 0..result.columns - 1 {
                    let base = result.minimum + Vec2::new(x as f32, z as f32) * VERTEX_SPACING;
                    let points = [
                        base,
                        base + Vec2::X * VERTEX_SPACING,
                        base + Vec2::Y * VERTEX_SPACING,
                        base + Vec2::splat(VERTEX_SPACING),
                    ];
                    let heights = result.corners(x, z);
                    result.subdivisions[z * result.columns + x] =
                        crate::terrain::terrain_cell_subdivisions(std::array::from_fn(|i| {
                            heights[i]
                                - terrain
                                    .water_surface_height(points[i].x, points[i].y)
                                    .unwrap()
                        }));
                }
            }
        }
        result
    }

    fn sample(lo: Vec2, hi: Vec2, origin: Vec2, yaw: f32, height: impl Fn(Vec2) -> f32) -> Self {
        let minimum = (lo / VERTEX_SPACING).floor() * VERTEX_SPACING;
        let maximum = (hi / VERTEX_SPACING).ceil() * VERTEX_SPACING;
        let size = ((maximum - minimum) / VERTEX_SPACING).round().as_uvec2() + UVec2::ONE;
        let columns = size.x.max(2) as usize;
        let rows = size.y.max(2) as usize;
        let mut heights = Vec::with_capacity(columns * rows);
        for z in 0..rows {
            for x in 0..columns {
                heights.push(height(
                    minimum + Vec2::new(x as f32, z as f32) * VERTEX_SPACING,
                ));
            }
        }
        Self {
            origin,
            yaw,
            minimum,
            columns,
            rows,
            heights,
            subdivisions: vec![1; columns * rows],
        }
    }

    fn corners(&self, x: usize, z: usize) -> [f32; 4] {
        let i = z * self.columns + x;
        [
            self.heights[i],
            self.heights[i + 1],
            self.heights[i + self.columns],
            self.heights[i + self.columns + 1],
        ]
    }

    fn height(&self, world: Vec2) -> f32 {
        let grid = (world - self.minimum) / VERTEX_SPACING;
        let x = (grid.x.floor().max(0.) as usize).min(self.columns - 2);
        let z = (grid.y.floor().max(0.) as usize).min(self.rows - 2);
        crate::terrain::rendered_cell_height(
            self.corners(x, z),
            (grid - Vec2::new(x as f32, z as f32)).clamp(Vec2::ZERO, Vec2::ONE),
            self.subdivisions[z * self.columns + x],
        )
    }

    pub(super) fn at(&self, p: Vec2, up: f32) -> Vec3 {
        let world = self.origin + local_to_world_xz(p, self.yaw);
        // Yard mesh roots carry yaw and XZ only. World-height vertices avoid
        // adding the house's old foundation height a second time after grading.
        Vec3::new(p.x, self.height(world) + up, p.y)
    }

    /// A convex counter-clockwise soil patch, fitted to accepted planting land.
    pub(super) fn patch(&self, mesh: &mut YardMesh, points: &[Vec2], color: Vec3) {
        if points.len() < 3 || points.len() > 8 {
            return;
        }
        let mut polygon = Polygon::default();
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for &p in points {
            let world = self.origin + local_to_world_xz(p, self.yaw);
            polygon.push(world);
            lo = lo.min(world);
            hi = hi.max(world);
        }
        let minimum = ((lo - self.minimum) / VERTEX_SPACING)
            .floor()
            .max(Vec2::ZERO)
            .as_uvec2();
        let maximum = ((hi - self.minimum) / VERTEX_SPACING)
            .floor()
            .max(Vec2::ZERO)
            .as_uvec2();
        for z in minimum.y as usize..=(maximum.y as usize).min(self.rows - 2) {
            for x in minimum.x as usize..=(maximum.x as usize).min(self.columns - 2) {
                let divisions = self.subdivisions[z * self.columns + x];
                let step = VERTEX_SPACING / divisions as f32;
                let base = self.minimum + Vec2::new(x as f32, z as f32) * VERTEX_SPACING;
                for iz in 0..divisions {
                    for ix in 0..divisions {
                        let p = base + Vec2::new(ix as f32, iz as f32) * step;
                        let corners = [
                            p,
                            p + Vec2::X * step,
                            p + Vec2::Y * step,
                            p + Vec2::splat(step),
                        ];
                        for ids in [[0, 1, 2], [1, 3, 2]] {
                            let triangle = ids.map(|i| corners[i]);
                            let mut clipped = polygon;
                            for i in 0..3 {
                                clipped = clipped.clip(triangle[i], triangle[(i + 1) % 3]);
                                if clipped.len < 3 {
                                    break;
                                }
                            }
                            let at = |world: Vec2| {
                                let p = world_to_local_xz(world - self.origin, self.yaw);
                                Vec3::new(p.x, self.height(world) + SOIL_LIFT, p.y)
                            };
                            for i in 1..clipped.len.saturating_sub(1) {
                                let [a, b, c] = [
                                    at(clipped.points[0]),
                                    at(clipped.points[i + 1]),
                                    at(clipped.points[i]),
                                ];
                                mesh.triangle(a, b, c, color);
                            }
                        }
                    }
                }
            }
        }
    }
}

// Eight convex soil points plus three triangle clips need at most eleven
// unique vertices. Scratch stays on the stack even at shore refinements.
#[derive(Clone, Copy)]
struct Polygon {
    points: [Vec2; 24],
    len: usize,
}
impl Default for Polygon {
    fn default() -> Self {
        Self {
            points: [Vec2::ZERO; 24],
            len: 0,
        }
    }
}
impl Polygon {
    fn push(&mut self, p: Vec2) {
        self.points[self.len] = p;
        self.len += 1;
    }
    fn clip(self, a: Vec2, b: Vec2) -> Self {
        let mut result = Self::default();
        if self.len == 0 {
            return result;
        }
        let side = |p: Vec2| (b - a).perp_dot(p - a);
        let mut previous = self.points[self.len - 1];
        let mut old = side(previous);
        for current in self.points[..self.len].iter().copied() {
            let next = side(current);
            if (old >= 0.) != (next >= 0.) {
                result.push(previous.lerp(current, old / (old - next)));
            }
            if next >= 0. {
                result.push(current);
            }
            previous = current;
            old = next;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn rotated_yard_soil_tracks_rendered_triangles_with_centimetre_clearance() {
        let points = [
            Vec2::new(-2.2, -0.4),
            Vec2::new(1.4, -0.6),
            Vec2::new(1.8, 0.5),
            Vec2::new(-2.0, 0.7),
        ];
        for yaw in [0., 0.37, -1.15] {
            for divisions in [1, 4] {
                let origin = Vec2::new(-63.4, 62.9);
                let mut ground = Ground::sample(
                    origin - Vec2::splat(8.),
                    origin + Vec2::splat(8.),
                    origin,
                    yaw,
                    |p| (p.x * 0.4).sin() * 0.35 + (p.y * 0.7).cos() * 0.3,
                );
                ground.subdivisions.fill(divisions);
                let mut mesh = YardMesh::default();
                ground.patch(&mut mesh, &points, Vec3::splat(0.4));
                let mesh = mesh.finish();
                let Some(VertexAttributeValues::Float32x3(vertices)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("missing soil vertices")
                };
                let mut area = 0.;
                for tri in vertices.chunks_exact(3) {
                    let [a, b, c] = [tri[0], tri[1], tri[2]].map(Vec3::from_array);
                    area += (b.xz() - a.xz()).perp_dot(c.xz() - a.xz()).abs() * 0.5;
                    for weights in [Vec3::splat(1. / 3.), Vec3::new(0.1, 0.3, 0.6), Vec3::X] {
                        let p = a * weights.x + b * weights.y + c * weights.z;
                        let height = ground.height(origin + local_to_world_xz(p.xz(), yaw));
                        assert!(
                            (p.y - height - SOIL_LIFT).abs() < 0.0001,
                            "yaw={yaw} divisions={divisions}"
                        );
                    }
                }
                let expected = (0..points.len())
                    .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
                    .sum::<f32>()
                    * 0.5;
                assert!(
                    (area - expected).abs() < 0.001,
                    "soil must cover each bed exactly once"
                );
            }
        }
    }
}
