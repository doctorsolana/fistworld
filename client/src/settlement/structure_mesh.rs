//! Closed vertex-coloured structural timber and masonry primitives.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct StructureMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl StructureMesh {
    pub(super) fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let first = self.positions.len() as u32;
        let linear = Color::srgb(color.x, color.y, color.z).to_linear();
        for point in [a, b, c] {
            self.positions.push(point.to_array());
            self.normals.push(normal.to_array());
            self.colors
                .push([linear.red, linear.green, linear.blue, 1.0]);
        }
        self.indices.extend([first, first + 1, first + 2]);
    }

    pub(super) fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
        self.triangle(a, b, c, color);
        self.triangle(a, c, d, color);
    }

    /// A solid rectangular beam between endpoint centres, including both caps.
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
        let vertices = [
            a - side - up,
            a + side - up,
            a + side + up,
            a - side + up,
            b - side - up,
            b + side - up,
            b + side + up,
            b - side + up,
        ];
        // The outward direction determines winding even for vertical braces.
        let center = (a + b) * 0.5;
        for [ia, ib, ic, id] in [
            [0, 1, 2, 3],
            [4, 7, 6, 5],
            [0, 4, 5, 1],
            [1, 5, 6, 2],
            [2, 6, 7, 3],
            [3, 7, 4, 0],
        ] {
            let [a, b, c, d] = [vertices[ia], vertices[ib], vertices[ic], vertices[id]];
            if (b - a).cross(c - a).dot((a + b + c + d) * 0.25 - center) < 0.0 {
                self.quad(d, c, b, a, color);
            } else {
                self.quad(a, b, c, d, color);
            }
        }
    }

    pub(super) fn stake(&mut self, base: Vec3, height: f32, radius: f32, color: Vec3) {
        const SIDES: usize = 6;
        let shoulder = base + Vec3::Y * (height - 0.3);
        let tip = base + Vec3::Y * height;
        for side in 0..SIDES {
            let angle = side as f32 * std::f32::consts::TAU / SIDES as f32;
            let next = (side + 1) as f32 * std::f32::consts::TAU / SIDES as f32;
            let offset = Vec3::new(angle.cos(), 0.0, angle.sin()) * radius;
            let next_offset = Vec3::new(next.cos(), 0.0, next.sin()) * radius;
            self.quad(
                base + next_offset,
                base + offset,
                shoulder + offset,
                shoulder + next_offset,
                color,
            );
            self.triangle(shoulder + next_offset, shoulder + offset, tip, color * 1.13);
            self.triangle(base, base + offset, base + next_offset, color * 0.8);
        }
    }

    pub(super) fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
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
    fn closed_beam_faces_point_outward() {
        let mut builder = StructureMesh::default();
        builder.beam(
            Vec3::new(-2.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            1.0,
            2.0,
            Vec3::ONE,
        );
        for (positions, normals) in builder
            .positions
            .chunks_exact(3)
            .zip(builder.normals.chunks_exact(3))
        {
            let center = positions.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 3.0;
            assert!(center.dot(Vec3::from_array(normals[0])) > 0.0);
        }
        assert_eq!(builder.indices.len(), 36);
    }
}
