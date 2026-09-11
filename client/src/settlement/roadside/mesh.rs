//! Opaque, textureless details combined into a single chunk mesh.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use shared::terrain::{WorldTerrain, VERTEX_SPACING};

use super::placement::{roll, Candidate, DetailKind};

/// A stone group is less than one 2 m terrain cell wide. Nine existing terrain
/// vertices cover it even across a cell/chunk edge; this stack-only stencil
/// lives for one mesh append and never becomes a streaming cache.
struct StoneGround {
    minimum: Vec2,
    heights: [f32; 9],
    subdivisions: [usize; 4],
}

impl StoneGround {
    fn sample(point: Vec2, radius: f32, height_at: impl Fn(Vec2) -> f32) -> Self {
        debug_assert!(radius * 2.0 < VERTEX_SPACING);
        let minimum = ((point - Vec2::splat(radius)) / VERTEX_SPACING).floor() * VERTEX_SPACING;
        Self {
            minimum,
            heights: std::array::from_fn(|i| {
                height_at(minimum + Vec2::new((i % 3) as f32, (i / 3) as f32) * VERTEX_SPACING)
            }),
            subdivisions: [1; 4],
        }
    }

    fn new(candidate: &Candidate, terrain: &WorldTerrain) -> Self {
        let mut ground = Self::sample(candidate.point, candidate.radius, |p| {
            terrain.get_height(p.x, p.y)
        });
        if terrain.water_level().is_some() {
            let signed: [f32; 9] = std::array::from_fn(|i| {
                let p = ground.minimum + Vec2::new((i % 3) as f32, (i / 3) as f32) * VERTEX_SPACING;
                terrain
                    .water_surface_height(p.x, p.y)
                    .map_or(f32::INFINITY, |water| ground.heights[i] - water)
            });
            for z in 0..2 {
                for x in 0..2 {
                    let i = z * 3 + x;
                    ground.subdivisions[z * 2 + x] = crate::terrain::terrain_cell_subdivisions([
                        signed[i],
                        signed[i + 1],
                        signed[i + 3],
                        signed[i + 4],
                    ]);
                }
            }
        }
        ground
    }

    fn height(&self, point: Vec2) -> f32 {
        let grid = (point - self.minimum) / VERTEX_SPACING;
        let cell = grid.floor().clamp(Vec2::ZERO, Vec2::ONE);
        let uv = (grid - cell).clamp(Vec2::ZERO, Vec2::ONE);
        let x = cell.x as usize;
        let z = cell.y as usize;
        let i = z * 3 + x;
        crate::terrain::rendered_cell_height(
            [
                self.heights[i],
                self.heights[i + 1],
                self.heights[i + 3],
                self.heights[i + 4],
            ],
            uv,
            self.subdivisions[z * 2 + x],
        )
    }
}

#[derive(Default)]
pub(super) struct RoadsideMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
}

impl RoadsideMesh {
    pub fn triangle_bound(kind: DetailKind) -> usize {
        match kind {
            DetailKind::Stone => 60,
            DetailKind::Bush => 236,
            DetailKind::Flowers => 680,
        }
    }

    fn triangle(&mut self, points: [Vec3; 3], color: Vec3) {
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let color = Color::srgb(color.x, color.y, color.z).to_linear();
        for point in points {
            self.positions.push(point.to_array());
            self.normals.push(normal.to_array());
            self.colors.push([color.red, color.green, color.blue, 1.0]);
        }
    }

    fn octahedron(&mut self, center: Vec3, radius: Vec3, color: Vec3) {
        let top = center + Vec3::Y * radius.y;
        let bottom = center - Vec3::Y * radius.y;
        let ring = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z].map(|p| center + p * radius);
        for i in 0..4 {
            let next = (i + 1) % 4;
            self.triangle([top, ring[next], ring[i]], color);
            self.triangle([bottom, ring[i], ring[next]], color * 0.84);
        }
    }

    /// Twenty faceted triangles form a round low crown or a flattened stone.
    /// Closed geometry keeps undersides visible without two-sided materials.
    fn ellipsoid(&mut self, center: Vec3, radius: Vec3, yaw: f32, color: Vec3) {
        self.mapped_ellipsoid(center, radius, yaw, color, |point| point);
    }

    fn mapped_ellipsoid(
        &mut self,
        center: Vec3,
        radius: Vec3,
        yaw: f32,
        color: Vec3,
        position: impl Fn(Vec3) -> Vec3,
    ) {
        let t = (1.0 + 5.0_f32.sqrt()) * 0.5;
        let base = [
            Vec3::new(-1., t, 0.),
            Vec3::new(1., t, 0.),
            Vec3::new(-1., -t, 0.),
            Vec3::new(1., -t, 0.),
            Vec3::new(0., -1., t),
            Vec3::new(0., 1., t),
            Vec3::new(0., -1., -t),
            Vec3::new(0., 1., -t),
            Vec3::new(t, 0., -1.),
            Vec3::new(t, 0., 1.),
            Vec3::new(-t, 0., -1.),
            Vec3::new(-t, 0., 1.),
        ];
        let rotation = Quat::from_rotation_y(yaw);
        let vertices = base.map(|p| position(center + rotation * (p.normalize() * radius)));
        let center = position(center);
        for [a, b, c] in [
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ] {
            let mut points = [vertices[a], vertices[b], vertices[c]];
            if (points[1] - points[0])
                .cross(points[2] - points[0])
                .dot((points[0] + points[1] + points[2]) / 3.0 - center)
                < 0.0
            {
                points.swap(1, 2);
            }
            self.triangle(points, color);
        }
    }

    fn tetrahedron(&mut self, vertices: [Vec3; 4], color: Vec3) {
        let center = vertices.into_iter().sum::<Vec3>() * 0.25;
        for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
            let mut points = [vertices[a], vertices[b], vertices[c]];
            if (points[1] - points[0])
                .cross(points[2] - points[0])
                .dot((points[0] + points[1] + points[2]) / 3.0 - center)
                < 0.0
            {
                points.swap(1, 2);
            }
            self.triangle(points, color);
        }
    }

    /// A pointed, folded solid leaf reads as a plant from both high and low angles.
    fn leaf(&mut self, base: Vec3, tip: Vec3, width: f32, color: Vec3) {
        let delta = tip - base;
        let side = Vec3::new(-delta.z, 0.0, delta.x).normalize_or(Vec3::X) * width;
        let middle = base.lerp(tip, 0.52);
        self.tetrahedron(
            [base, tip, middle + side + Vec3::Y * 0.018, middle - side],
            color,
        );
    }

    fn blossom(&mut self, head: Vec3, radius: f32, yaw: f32, yellow: bool) {
        let color = if yellow {
            Vec3::new(1.0, 0.86, 0.16)
        } else {
            Vec3::new(1.0, 0.99, 0.94)
        };
        for petal in 0..5 {
            let angle = yaw + petal as f32 * std::f32::consts::TAU / 5.0;
            let out = Vec3::new(angle.cos(), 0.0, angle.sin());
            let side = Vec3::new(-out.z, 0.0, out.x);
            let points = [
                head + out * radius * 0.10,
                head + out * radius * 0.61 + side * radius * 0.29,
                head + out * radius + Vec3::Y * 0.014,
                head + out * radius * 0.61 - side * radius * 0.29,
            ];
            // Thin opaque petals have both surfaces, independent of view angle.
            self.triangle([points[0], points[1], points[2]], color);
            self.triangle([points[0], points[2], points[3]], color);
            self.triangle([points[2], points[1], points[0]], color * 0.86);
            self.triangle([points[3], points[2], points[0]], color * 0.86);
        }
        self.octahedron(
            head + Vec3::Y * 0.012,
            Vec3::new(radius * 0.23, 0.017, radius * 0.23),
            if yellow {
                Vec3::new(0.52, 0.34, 0.08)
            } else {
                Vec3::new(0.93, 0.69, 0.15)
            },
        );
    }

    pub fn append(&mut self, candidate: &Candidate, terrain: &WorldTerrain, origin: Vec3) {
        if candidate.kind == DetailKind::Stone {
            let ground = StoneGround::new(candidate, terrain);
            self.append_with_ground(candidate, origin, |point| ground.height(point));
            return;
        }
        self.append_with_ground(candidate, origin, |point| {
            terrain.get_height(point.x, point.y)
        });
    }

    fn append_with_ground(
        &mut self,
        candidate: &Candidate,
        origin: Vec3,
        height_at: impl Fn(Vec2) -> f32,
    ) {
        let seed = candidate.seed;
        let at = candidate.point;
        let ground = Vec3::new(at.x, height_at(at), at.y) - origin;
        match candidate.kind {
            DetailKind::Stone => {
                let height = 0.18 + roll(seed, 9) * 0.08;
                let tone = 0.90 + roll(seed, 10) * 0.12;
                let angle = roll(seed, 11) * std::f32::consts::TAU;
                // Every vertex follows the rendered ground, not a horizontal
                // plane at the stone centre. Equators/lower faces are embedded;
                // raised upper facets stay below 0.20 m above local ground.
                let grounded = |point: Vec3| {
                    let world = point.xz() + origin.xz();
                    point + Vec3::Y * (height_at(world) - ground.y - origin.y)
                };
                self.mapped_ellipsoid(
                    ground - Vec3::Y * 0.035,
                    Vec3::new(candidate.radius * 0.70, height, candidate.radius * 0.54),
                    angle,
                    Vec3::new(0.78, 0.76, 0.67) * tone,
                    grounded,
                );
                for i in 0..2 {
                    let angle = angle + i as f32 * 2.65;
                    let p = at + Vec2::new(angle.cos(), angle.sin()) * candidate.radius * 0.70;
                    let base = Vec3::new(p.x, ground.y + origin.y, p.y) - origin;
                    self.mapped_ellipsoid(
                        base - Vec3::Y * 0.025,
                        Vec3::new(
                            candidate.radius * 0.25,
                            height * 0.55,
                            candidate.radius * 0.22,
                        ),
                        angle,
                        Vec3::new(0.73, 0.72, 0.64) * tone,
                        grounded,
                    );
                }
            }
            DetailKind::Bush => {
                // Rounded outer lobes and a slightly taller centre form a low
                // dome, keeping the established horizontal planting footprint.
                for i in 0..7 {
                    let angle = roll(seed, 18) * std::f32::consts::TAU + i as f32 * 2.40;
                    let distance = if i == 0 { 0.0 } else { 0.45 };
                    let offset = Vec2::new(angle.cos(), angle.sin()) * candidate.radius * distance;
                    let p = at + offset;
                    let base = Vec3::new(p.x, height_at(p), p.y) - origin;
                    let radius = candidate.radius * (0.25 + roll(seed, 20 + i) * 0.09);
                    let height = 0.40 + roll(seed, 23 + i) * 0.11 + if i == 0 { 0.06 } else { 0.0 };
                    let tone = 0.89 + roll(seed, 27 + i) * 0.15;
                    self.ellipsoid(
                        base + Vec3::Y * height * 0.52,
                        Vec3::new(radius, height, radius * 0.92),
                        angle,
                        Vec3::new(0.39, 0.55, 0.22) * tone,
                    );
                }
                for i in 0..24 {
                    let angle = roll(seed, 140 + i) * std::f32::consts::TAU;
                    let direction = Vec2::new(angle.cos(), angle.sin());
                    let p = at + direction * candidate.radius * roll(seed, 170 + i).sqrt() * 0.67;
                    let base = Vec3::new(p.x, height_at(p) - 0.015, p.y) - origin;
                    let tip = base
                        + Vec3::new(
                            direction.x * 0.19,
                            0.50 + roll(seed, 200 + i) * 0.26,
                            direction.y * 0.19,
                        );
                    self.leaf(
                        base,
                        tip,
                        0.08 + roll(seed, 230 + i) * 0.065,
                        Vec3::new(0.40, 0.56, 0.23) * (0.86 + roll(seed, 260 + i) * 0.20),
                    );
                }
            }
            DetailKind::Flowers => {
                let count = 13 + (roll(seed, 30) * 5.0) as u64;
                let yellow = roll(seed, 31) < 0.45;
                for i in 0..count {
                    let angle = roll(seed, 40) * std::f32::consts::TAU + i as f32 * 2.40;
                    let distance = (i as f32 / count as f32).sqrt() * candidate.radius * 0.72;
                    let p = at + Vec2::new(angle.cos(), angle.sin()) * distance;
                    let base = Vec3::new(p.x, height_at(p) - 0.025, p.y) - origin;
                    let height = 0.22 + roll(seed, 80 + i) * 0.11;
                    let head = base + Vec3::new(0.018 * angle.cos(), height, 0.018 * angle.sin());
                    let green = Vec3::new(0.32, 0.44, 0.18);
                    self.tetrahedron(
                        [
                            base + Vec3::X * 0.016,
                            base - Vec3::X * 0.009 + Vec3::Z * 0.014,
                            base - Vec3::X * 0.009 - Vec3::Z * 0.014,
                            head,
                        ],
                        green,
                    );
                    let out = Vec3::new(angle.cos(), 0.0, angle.sin());
                    let leaf_base = base + Vec3::Y * 0.065;
                    self.leaf(
                        leaf_base,
                        leaf_base + out * 0.15 + Vec3::Y * 0.095,
                        0.045,
                        green * 1.08,
                    );
                    self.leaf(
                        leaf_base,
                        leaf_base - out * 0.13 + Vec3::Y * 0.07,
                        0.05,
                        green,
                    );
                    self.blossom(head, 0.19 + roll(seed, 100 + i) * 0.05, angle, yellow);
                }
            }
        }
    }

    pub fn triangles(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheap_closed_crowns_have_outward_faces_and_opaque_colours() {
        let mut mesh = RoadsideMesh::default();
        mesh.ellipsoid(Vec3::ZERO, Vec3::ONE, 0.7, Vec3::splat(0.5));
        assert_eq!(mesh.triangles(), 20);
        for (points, normals) in mesh
            .positions
            .chunks_exact(3)
            .zip(mesh.normals.chunks_exact(3))
        {
            let center = points.iter().copied().map(Vec3::from_array).sum::<Vec3>() / 3.0;
            assert!(center.dot(Vec3::from_array(normals[0])) > 0.0);
        }
        assert!(mesh.colors.iter().all(|c| c[3] == 1.0));
    }

    #[test]
    fn details_fit_their_reserved_footprints_and_stay_low() {
        for (kind, radius, maximum_height) in [
            (DetailKind::Stone, 0.32, 0.20),
            (DetailKind::Flowers, 1.10, 0.35),
            (DetailKind::Bush, 1.15, 0.80),
            (DetailKind::Bush, 1.70, 0.80),
        ] {
            for seed in 0..100 {
                let mut mesh = RoadsideMesh::default();
                mesh.append_with_ground(
                    &Candidate {
                        point: Vec2::ZERO,
                        radius,
                        seed,
                        kind,
                    },
                    Vec3::ZERO,
                    |_| 0.0,
                );
                assert!(mesh.triangles() <= RoadsideMesh::triangle_bound(kind));
                for position in mesh.positions.iter().copied().map(Vec3::from_array) {
                    assert!(
                        position.xz().length() <= radius + 0.001,
                        "{kind:?}: {position}"
                    );
                    assert!(position.y <= maximum_height, "{kind:?}: {position}");
                }
                assert!(mesh.positions.iter().any(|position| position[1] < 0.0));
            }
        }
    }

    #[test]
    fn stone_lower_faces_remain_bedded_on_slopes_and_convex_ground_across_chunk_edges() {
        let at = Vec2::new(63.95, -0.04);
        let origin = Vec3::new(64.0, 3.0, -64.0);
        for radius in [0.32, 0.57] {
            for surface in 0..3 {
                let height = |p: Vec2| {
                    let local = p - at;
                    7.0 + match surface {
                        0 => local.x * 0.14,
                        1 => local.x * -0.14,
                        _ => local.length_squared() * 0.07,
                    }
                };
                let mut ground = StoneGround::sample(at, radius, height);
                for subdivisions in [1, 4] {
                    ground.subdivisions.fill(subdivisions);
                    for seed in 0..24 {
                        let candidate = Candidate {
                            point: at,
                            radius,
                            seed,
                            kind: DetailKind::Stone,
                        };
                        let mut flat = RoadsideMesh::default();
                        flat.append_with_ground(&candidate, Vec3::ZERO, |_| 0.0);
                        let mut mesh = RoadsideMesh::default();
                        mesh.append_with_ground(&candidate, origin, |p| ground.height(p));
                        assert_eq!(mesh.triangles(), 60);
                        let mut lower_faces = 0;
                        for (flat, triangle) in flat
                            .positions
                            .chunks_exact(3)
                            .zip(mesh.positions.chunks_exact(3))
                        {
                            let points: [Vec3; 3] =
                                std::array::from_fn(|i| Vec3::from_array(triangle[i]) + origin);
                            for p in points {
                                assert!(p.xz().distance(at) <= radius + 0.001);
                                assert!(p.y - ground.height(p.xz()) <= 0.20);
                            }
                            if !flat.iter().all(|p| p[1] < -0.001) {
                                continue;
                            }
                            lower_faces += 1;
                            for weights in [
                                Vec3::X,
                                Vec3::Y,
                                Vec3::Z,
                                Vec3::new(0.5, 0.5, 0.0),
                                Vec3::new(0.0, 0.5, 0.5),
                                Vec3::new(0.5, 0.0, 0.5),
                                Vec3::splat(1.0 / 3.0),
                                Vec3::new(0.1, 0.3, 0.6),
                            ] {
                                let p = points[0] * weights.x
                                    + points[1] * weights.y
                                    + points[2] * weights.z;
                                let clearance = p.y - ground.height(p.xz());
                                assert!(
                                    clearance < 0.001,
                                    "floating lower face: r={radius} surface={surface} sub={subdivisions} seed={seed} clearance={clearance}"
                                );
                            }
                        }
                        assert!(lower_faces >= 12);
                    }
                }
            }
        }
    }

    #[test]
    fn independent_stone_stencils_agree_where_they_overlap_and_follow_terrain_changes() {
        let height = |p: Vec2| 5.0 + (p.x * 0.13).sin() * 0.3 + (p.y * 0.19).cos() * 0.2;
        let west = StoneGround::sample(Vec2::new(63.4, -0.3), 0.57, height);
        let east = StoneGround::sample(Vec2::new(64.6, 0.3), 0.57, height);
        for z in [-0.2, 0.0, 0.2] {
            for x in [64.0, 64.1, 65.2] {
                let p = Vec2::new(x, z);
                assert!((west.height(p) - east.height(p)).abs() < 0.00001);
            }
        }
        let edited = StoneGround::sample(Vec2::new(64.6, 0.3), 0.57, |p| height(p) + 0.4);
        assert!(
            (edited.height(Vec2::new(64.0, 0.0)) - east.height(Vec2::new(64.0, 0.0)) - 0.4).abs()
                < 0.00001
        );
    }
}
