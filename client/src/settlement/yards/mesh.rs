//! Small opaque meshes assembled once per household, not one entity per object.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct YardMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl YardMesh {
    pub(super) fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let first = self.positions.len() as u32;
        let color = Color::srgb(color.x, color.y, color.z).to_linear();
        for point in [a, b, c] {
            self.positions.push(point.to_array());
            self.normals.push(normal.to_array());
            self.colors.push([color.red, color.green, color.blue, 1.]);
        }
        self.indices.extend([first, first + 1, first + 2]);
    }

    pub(super) fn quad(&mut self, points: [Vec3; 4], color: Vec3) {
        let [a, b, c, d] = points;
        self.triangle(a, b, c, color);
        self.triangle(a, c, d, color);
    }

    /// Closed beam with outward winding even for diagonal posts and braces.
    pub(super) fn beam(&mut self, a: Vec3, b: Vec3, width: f32, depth: f32, color: Vec3) {
        let along = (b - a).normalize_or_zero();
        if along == Vec3::ZERO {
            return;
        }
        let reference = if along.y.abs() > 0.95 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let side = along.cross(reference).normalize() * width * 0.5;
        let up = side.normalize().cross(along) * depth * 0.5;
        let v = [
            a - side - up,
            a + side - up,
            a + side + up,
            a - side + up,
            b - side - up,
            b + side - up,
            b + side + up,
            b - side + up,
        ];
        let center = (a + b) * 0.5;
        for [ia, ib, ic, id] in [
            [0, 1, 2, 3],
            [4, 7, 6, 5],
            [0, 4, 5, 1],
            [1, 5, 6, 2],
            [2, 6, 7, 3],
            [3, 7, 4, 0],
        ] {
            let [a, b, c, d] = [v[ia], v[ib], v[ic], v[id]];
            if (b - a).cross(c - a).dot((a + b + c + d) * 0.25 - center) < 0.0 {
                self.quad([d, c, b, a], color);
            } else {
                self.quad([a, b, c, d], color);
            }
        }
    }

    /// A closed eight-triangle foliage/vegetable crown. Height and width can
    /// differ, so small round cabbages and leafy flower bases share topology.
    pub(super) fn crown(&mut self, center: Vec3, radius: Vec3, color: Vec3, yaw: f32) {
        let turn = Quat::from_rotation_y(yaw);
        let top = center + Vec3::Y * radius.y;
        let bottom = center - Vec3::Y * radius.y;
        let ring =
            [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z].map(|p| center + turn * (p * radius));
        for i in 0..4 {
            let next = (i + 1) % 4;
            self.triangle(top, ring[next], ring[i], color * (1.0 + i as f32 * 0.025));
            self.triangle(bottom, ring[i], ring[next], color * 0.86);
        }
    }

    /// Four outward faces make a folded leaf/petal with a visible silhouette
    /// from either side. Reuses vertex colours and the yard's opaque material.
    pub(super) fn leaf(&mut self, base: Vec3, tip: Vec3, width: f32, color: Vec3) {
        let direction = (tip - base).normalize_or_zero();
        if direction == Vec3::ZERO {
            return;
        }
        let side = Vec3::new(-direction.z, 0.0, direction.x).normalize_or(Vec3::X);
        let middle = base.lerp(tip, 0.52);
        let ridge = side.cross(direction) * width * 0.32;
        let points = [
            base,
            tip,
            middle + side * width,
            middle - side * width + ridge,
        ];
        let center = points.into_iter().sum::<Vec3>() * 0.25;
        for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
            let [a, mut b, mut c] = [points[a], points[b], points[c]];
            if (b - a).cross(c - a).dot((a + b + c) / 3.0 - center) < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            self.triangle(a, b, c, color);
        }
    }

    /// Six-sided log, including pale cut ends; caps meet the bark ring exactly.
    pub(super) fn log(&mut self, a: Vec3, b: Vec3, radius: f32) {
        let along = (b - a).normalize();
        let side = along.cross(Vec3::Y).normalize();
        let up = side.cross(along);
        for step in 0..6 {
            let point = |i: usize| {
                let angle = i as f32 * std::f32::consts::TAU / 6.0;
                (side * angle.cos() + up * angle.sin()) * radius
            };
            let p = point(step);
            let q = point(step + 1);
            self.quad([a + p, b + p, b + q, a + q], Vec3::new(0.32, 0.20, 0.105));
            self.triangle(a, a + p, a + q, Vec3::new(0.66, 0.48, 0.28));
            self.triangle(b, b + q, b + p, Vec3::new(0.62, 0.43, 0.24));
        }
    }

    pub(super) fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Keep each complete decorative group or omit it. Never cut a fence, a
    /// supported rack, or half a plant to satisfy a presentation budget.
    pub(super) fn append_with_budget(&mut self, mut other: Self, budget: usize) -> bool {
        if self.triangle_count() + other.triangle_count() > budget {
            return false;
        }
        let first = self.positions.len() as u32;
        self.positions.append(&mut other.positions);
        self.normals.append(&mut other.normals);
        self.colors.append(&mut other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + first));
        true
    }

    /// Squat six-sided foliage with a broad top instead of an octahedron's
    /// diamond point. Beds read as overlapping leafy heads at town distance.
    pub(super) fn leafy_head(&mut self, center: Vec3, radius: Vec3, color: Vec3, yaw: f32) {
        let ring = |i: usize, scale: f32, y: f32| {
            let angle = yaw + i as f32 * std::f32::consts::TAU / 6.;
            center
                + Vec3::new(
                    angle.cos() * radius.x * scale,
                    y * radius.y,
                    angle.sin() * radius.z * scale,
                )
        };
        for i in 0..6 {
            self.quad(
                [
                    ring(i, 1., 0.),
                    ring(i, 0.55, 0.85),
                    ring(i + 1, 0.55, 0.85),
                    ring(i + 1, 1., 0.),
                ],
                color * (0.96 + i as f32 * 0.012),
            );
            self.triangle(
                center + Vec3::Y * radius.y,
                ring(i + 1, 0.55, 0.85),
                ring(i, 0.55, 0.85),
                color * 1.08,
            );
            self.triangle(
                center - Vec3::Y * radius.y * 0.8,
                ring(i, 1., 0.),
                ring(i + 1, 1., 0.),
                color * 0.85,
            );
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub(super) fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_leaves_have_closed_outward_geometry_in_upright_and_flat_poses() {
        for tip in [Vec3::Y * 0.4, Vec3::new(0.3, 0.2, 0.1), Vec3::X * 0.3] {
            let mut mesh = YardMesh::default();
            mesh.leaf(Vec3::ZERO, tip, 0.1, Vec3::splat(0.5));
            assert_eq!(mesh.indices.len(), 12);
            let center = mesh
                .positions
                .iter()
                .copied()
                .map(Vec3::from_array)
                .sum::<Vec3>()
                / 12.0;
            for (triangle, normal) in mesh
                .positions
                .chunks_exact(3)
                .zip(mesh.normals.chunks_exact(3))
            {
                let face = triangle.iter().copied().map(Vec3::from_array).sum::<Vec3>() / 3.0;
                assert!((face - center).dot(Vec3::from_array(normal[0])) > 0.0);
            }
        }
    }
}
