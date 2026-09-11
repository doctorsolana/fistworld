//! Thin crop soil follows the triangles the client actually renders. Sampling
//! the smooth height field on a different, rotated lattice lets terrain poke
//! through the soil, even when every sampled vertex appears above ground.

use super::*;
use shared::terrain::VERTEX_SPACING;

pub(super) const SOIL_LIFT: f32 = 0.024;

/// A small field-local cache of the existing terrain vertex heights. Constructed
/// only on a parcel/terrain rebuild; stalks do not repeatedly sample world data.
pub(super) struct FieldGround {
    minimum: Vec2,
    columns: usize,
    rows: usize,
    heights: Vec<f32>,
    subdivisions: Vec<usize>,
}

impl FieldGround {
    pub(super) fn new(
        shape: &FarmFieldShape,
        origin: Vec3,
        yaw: f32,
        terrain: &WorldTerrain,
    ) -> Self {
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for row in &shape.sections {
            for x in [row.left, row.right] {
                let world =
                    origin.xz() + shared::rotation::local_to_world_xz(Vec2::new(x, row.z), yaw);
                lo = lo.min(world);
                hi = hi.max(world);
            }
        }
        let mut ground = Self::sample(lo, hi, |p| terrain.get_height(p.x, p.y));
        if terrain.water_level().is_some() {
            let mut signed = Vec::with_capacity(ground.heights.len());
            for z in 0..ground.rows {
                for x in 0..ground.columns {
                    let p = ground.minimum + Vec2::new(x as f32, z as f32) * VERTEX_SPACING;
                    signed.push(
                        ground.heights[z * ground.columns + x]
                            - terrain.water_surface_height(p.x, p.y).unwrap(),
                    );
                }
            }
            for z in 0..ground.rows - 1 {
                for x in 0..ground.columns - 1 {
                    let i = z * ground.columns + x;
                    ground.subdivisions[i] = crate::terrain::terrain_cell_subdivisions([
                        signed[i],
                        signed[i + 1],
                        signed[i + ground.columns],
                        signed[i + ground.columns + 1],
                    ]);
                }
            }
        }
        ground
    }

    fn sample(lo: Vec2, hi: Vec2, height: impl Fn(Vec2) -> f32) -> Self {
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
            minimum,
            columns,
            rows,
            heights,
            subdivisions: vec![1; columns * rows],
        }
    }

    fn cell(&self, world: Vec2) -> (usize, usize, Vec2) {
        let grid = (world - self.minimum) / VERTEX_SPACING;
        let x = (grid.x.floor().max(0.) as usize).min(self.columns - 2);
        let z = (grid.y.floor().max(0.) as usize).min(self.rows - 2);
        (
            x,
            z,
            (grid - Vec2::new(x as f32, z as f32)).clamp(Vec2::ZERO, Vec2::ONE),
        )
    }

    fn corners(&self, x: usize, z: usize) -> [f32; 4] {
        [
            self.heights[z * self.columns + x],
            self.heights[z * self.columns + x + 1],
            self.heights[(z + 1) * self.columns + x],
            self.heights[(z + 1) * self.columns + x + 1],
        ]
    }

    pub(super) fn height(&self, world: Vec2) -> f32 {
        let (x, z, uv) = self.cell(world);
        let heights = self.corners(x, z);
        let subdivisions = self.subdivisions[z * self.columns + x];
        crate::terrain::rendered_cell_height(heights, uv, subdivisions)
    }

    pub(super) fn soil(
        &self,
        shape: &FarmFieldShape,
        origin: Vec3,
        yaw: f32,
        farm_offset: Vec2,
    ) -> FieldMesh {
        let mut mesh = FieldMesh::default();
        let local = |p| shared::rotation::world_to_local_xz(p - origin.xz(), yaw);
        for pair in shape.sections.windows(2) {
            let parcel = [
                Vec2::new(pair[0].left, pair[0].z),
                Vec2::new(pair[0].right, pair[0].z),
                Vec2::new(pair[1].right, pair[1].z),
                Vec2::new(pair[1].left, pair[1].z),
            ];
            let mut lo = Vec2::splat(f32::INFINITY);
            let mut hi = Vec2::splat(f32::NEG_INFINITY);
            for p in parcel {
                let world = origin.xz() + shared::rotation::local_to_world_xz(p, yaw);
                lo = lo.min(world);
                hi = hi.max(world);
            }
            let (x0, z0, _) = self.cell(lo);
            let (x1, z1, _) = self.cell(hi);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let base = self.minimum + Vec2::new(x as f32, z as f32) * VERTEX_SPACING;
                    let subdivisions = self.subdivisions[z * self.columns + x];
                    let step = VERTEX_SPACING / subdivisions as f32;
                    for sub_z in 0..subdivisions {
                        for sub_x in 0..subdivisions {
                            let base = base + Vec2::new(sub_x as f32, sub_z as f32) * step;
                            let corners = [
                                base,
                                base + Vec2::X * step,
                                base + Vec2::Y * step,
                                base + Vec2::splat(step),
                            ];
                            for triangle in [[0, 1, 2], [1, 3, 2]] {
                                let triangle = triangle.map(|i| local(corners[i]));
                                let mut polygon = ClipPolygon::new(parcel);
                                for i in 0..3 {
                                    polygon = clip(polygon, triangle[i], triangle[(i + 1) % 3]);
                                    if polygon.len() < 3 {
                                        break;
                                    }
                                }
                                if polygon.len() < 3 {
                                    continue;
                                }
                                let middle =
                                    polygon.iter().copied().sum::<Vec2>() / polygon.len() as f32;
                                let shade = 0.96
                                    + variation(
                                        middle.x + farm_offset.x,
                                        middle.y + farm_offset.y,
                                        3.,
                                    ) * 0.04;
                                let color = Vec3::new(0.61, 0.46, 0.245) * shade;
                                let point = |p: Vec2| {
                                    let world =
                                        origin.xz() + shared::rotation::local_to_world_xz(p, yaw);
                                    Vec3::new(p.x, self.height(world) - origin.y + SOIL_LIFT, p.y)
                                };
                                for i in 1..polygon.len() - 1 {
                                    mesh.triangle(
                                        point(polygon[0]),
                                        point(polygon[i + 1]),
                                        point(polygon[i]),
                                        color,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        mesh
    }
}

/// A convex quad clipped by three half-planes has at most seven distinct vertices.
/// Spare slots retain coincident edge intersections without changing the old
/// floating-point clipping sequence. All per-candidate scratch stays on the stack.
struct ClipPolygon {
    points: [Vec2; 16],
    len: usize,
}

impl ClipPolygon {
    fn new(points: [Vec2; 4]) -> Self {
        let mut result = Self {
            points: [Vec2::ZERO; 16],
            len: 4,
        };
        result.points[..4].copy_from_slice(&points);
        result
    }

    fn push(&mut self, point: Vec2) {
        self.points[self.len] = point;
        self.len += 1;
    }
}

impl std::ops::Deref for ClipPolygon {
    type Target = [Vec2];
    fn deref(&self) -> &Self::Target {
        &self.points[..self.len]
    }
}

fn clip(polygon: ClipPolygon, a: Vec2, b: Vec2) -> ClipPolygon {
    let mut out = ClipPolygon {
        points: [Vec2::ZERO; 16],
        len: 0,
    };
    if polygon.is_empty() {
        return out;
    }
    let side = |p: Vec2| (b - a).perp_dot(p - a);
    let mut previous = *polygon.last().unwrap();
    let mut old = side(previous);
    for current in polygon.iter().copied() {
        let next = side(current);
        if (old >= 0.) != (next >= 0.) {
            out.push(previous.lerp(current, (old / (old - next)).clamp(0., 1.)));
        }
        if next >= 0. {
            out.push(current);
        }
        previous = current;
        old = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_contact_uses_the_rendered_cell_diagonal_including_negative_world_coordinates() {
        let ground = FieldGround::sample(Vec2::splat(-2.), Vec2::splat(2.), |p| p.x * p.y);
        // The smooth bilinear height here is 0.25; the actual first rendered
        // triangle is flat because three corners have zero height.
        assert_eq!(ground.height(Vec2::splat(0.5)), 0.);
        assert_eq!(ground.height(Vec2::splat(1.5)), 2.);
        assert_eq!(ground.height(Vec2::new(-1.5, 1.5)), -3.);
        for p in [Vec2::ZERO, Vec2::splat(2.), Vec2::new(-2., 2.)] {
            assert_eq!(ground.height(p), p.x * p.y);
        }
    }

    #[test]
    fn rotated_soil_and_worker_split_stay_exactly_above_render_triangles() {
        let shape = FarmFieldShape::legacy_rectangle();
        for subdivisions in [
            1,
            crate::terrain::terrain_cell_subdivisions([-0.1, 0.2, 0.8, 1.1]),
        ] {
            for yaw in [0., 0.37, 1.13, -0.71] {
                let origin = Vec3::new(-63.4, 3.7, 62.9);
                let height = |p: Vec2| (p.x * 0.43).sin() * 0.45 + (p.y * 0.51).cos() * 0.36;
                let mut ground = FieldGround::sample(
                    origin.xz() - Vec2::splat(16.),
                    origin.xz() + Vec2::splat(16.),
                    height,
                );
                ground.subdivisions.fill(subdivisions);
                let mesh = ground.soil(&shape, origin, yaw, Vec2::new(-4., 9.));
                let area = mesh
                    .positions
                    .chunks_exact(3)
                    .map(|t| {
                        let a = Vec3::from_array(t[0]).xz();
                        let b = Vec3::from_array(t[1]).xz();
                        let c = Vec3::from_array(t[2]).xz();
                        (b - a).perp_dot(c - a).abs() * 0.5
                    })
                    .sum::<f32>();
                assert!(
                    (area - shape.area()).abs() < 0.01,
                    "soil lost or duplicated accepted ground: {area}"
                );
                assert!(!mesh.positions.is_empty());
                for triangle in mesh.positions.chunks_exact(3) {
                    let a = Vec3::from_array(triangle[0]);
                    let b = Vec3::from_array(triangle[1]);
                    let c = Vec3::from_array(triangle[2]);
                    for weights in [
                        Vec3::new(1., 0., 0.),
                        Vec3::new(0., 0.5, 0.5),
                        Vec3::splat(1. / 3.),
                        Vec3::new(0.1, 0.3, 0.6),
                    ] {
                        let p = a * weights.x + b * weights.y + c * weights.z;
                        let world = origin.xz() + shared::rotation::local_to_world_xz(p.xz(), yaw);
                        let clearance = p.y + origin.y - ground.height(world);
                        assert!(
                            (clearance - SOIL_LIFT).abs() < 0.0001,
                            "yaw={yaw} clearance={clearance}"
                        );
                        assert!(shape.contains_local_point(p.xz(), 0.001));
                    }
                }
                assert!(mesh.wind_weights.iter().all(|w| w[0] == 0.));
            }
        }
    }
}
