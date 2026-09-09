//! Closed, vertex-coloured masonry and timber for terrain-following defenses.
//!
//! Geometry is assembled only when a replicated section changes. Each section
//! is one mesh/material, including its braces and stone courses; individual
//! stones and palisade stakes are not ECS entities.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct MasonryMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MasonryMesh {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3) {
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

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3) {
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

    fn stake(&mut self, base: Vec3, height: f32, radius: f32, color: Vec3) {
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

/// `a` and `b` are terrain-grade endpoints in mesh-local space. The shared
/// contract supplies height/thickness, keeping the silhouette and blocker in
/// agreement. The continuous timber backing closes cracks between faceted logs.
pub(super) fn wall(a: Vec3, b: Vec3, height: f32, thickness: f32, stone: bool) -> Mesh {
    let mut mesh = MasonryMesh::default();
    let length = a.xz().distance(b.xz());
    let direction = (b - a).normalize_or_zero();
    let outward = Vec3::new(-direction.z, 0.0, direction.x).normalize_or_zero();
    let base = -0.20;
    if stone {
        let mortar = Vec3::new(0.36, 0.35, 0.30);
        mesh.beam(
            a + Vec3::Y * ((height + base) * 0.5),
            b + Vec3::Y * ((height + base) * 0.5),
            thickness,
            height - base,
            mortar,
        );
        let rows = (height / 0.65).ceil() as usize;
        let row_height = height / rows as f32;
        // Raised courses overlap the solid mortar core, preventing daylight
        // gaps while avoiding a coplanar second face.
        for row in 0..rows {
            let count = (length / 1.1).ceil().max(1.0) as usize;
            for index in 0..=count {
                let stagger = if row % 2 == 0 { 0.0 } else { -0.5 };
                let from = ((index as f32 + stagger) / count as f32).clamp(0.0, 1.0);
                let to = ((index as f32 + 1.0 + stagger) / count as f32).clamp(0.0, 1.0);
                if to - from < 0.02 {
                    continue;
                }
                let color = Vec3::new(0.55, 0.53, 0.45)
                    * (0.9 + ((row * 11 + index * 7) % 9) as f32 * 0.022);
                let y = row as f32 * row_height + row_height * 0.5;
                for side in [-1.0, 1.0] {
                    let offset = outward * side * (thickness * 0.5 + 0.015) + Vec3::Y * y;
                    mesh.beam(
                        a.lerp(b, from) + offset + direction * 0.012,
                        a.lerp(b, to) + offset - direction * 0.012,
                        0.10,
                        row_height - 0.025,
                        color,
                    );
                }
            }
        }
        mesh.beam(
            a + Vec3::Y * height,
            b + Vec3::Y * height,
            thickness + 0.12,
            0.22,
            Vec3::new(0.65, 0.62, 0.53),
        );
        let merlons = (length / 1.6).ceil().max(1.0) as usize;
        for index in 0..merlons {
            let from = index as f32 / merlons as f32;
            let to = (index as f32 + 0.52) / merlons as f32;
            mesh.beam(
                a.lerp(b, from) + Vec3::Y * (height + 0.42),
                a.lerp(b, to) + Vec3::Y * (height + 0.42),
                thickness + 0.06,
                0.7,
                Vec3::new(0.59, 0.56, 0.48),
            );
        }
    } else {
        let wood = Vec3::new(0.40, 0.25, 0.12);
        mesh.beam(
            a + Vec3::Y * (height * 0.45),
            b + Vec3::Y * (height * 0.45),
            thickness * 0.65,
            height * 0.9 + 0.4,
            wood * 0.82,
        );
        let count = (length / (thickness * 0.82)).ceil().max(1.0) as usize;
        for index in 0..=count {
            let at = a.lerp(b, index as f32 / count as f32) + Vec3::Y * base;
            let variation = (index % 5) as f32 * 0.026;
            mesh.stake(
                at,
                height - base - variation,
                thickness * 0.5,
                wood * (0.91 + variation),
            );
        }
        for side in [-1.0, 1.0] {
            for y in [0.8, height - 0.65] {
                let offset = outward * side * thickness * 0.48 + Vec3::Y * y;
                mesh.beam(a + offset, b + offset, 0.15, 0.2, wood * 0.7);
            }
        }
    }
    mesh.finish()
}

/// Open gatehouse: jambs sit OUTSIDE the clear span, with a closed-backed roof
/// and overhead lintel. It never places a decorative solid leaf across a route.
pub(super) fn gate(a: Vec3, b: Vec3, material: shared::components::FortificationMaterial) -> Mesh {
    let height = material.gate_clear_height() + 0.2;
    let thickness = material.thickness();
    let stone = material == shared::components::FortificationMaterial::Stone;
    let mut mesh = MasonryMesh::default();
    let along = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or_zero();
    let outward = Vec3::new(-along.z, 0.0, along.x).normalize_or_zero();
    let wood = Vec3::new(0.36, 0.21, 0.095);
    let jamb_width = material.gate_post_width();
    let top = a.y.max(b.y) + height;
    for (point, sign) in [(a, -1.0), (b, 1.0)] {
        let at = point + along * sign * material.gate_post_offset();
        let color = if stone {
            Vec3::new(0.59, 0.56, 0.47)
        } else {
            wood
        };
        let midpoint = Vec3::new(at.x, (at.y + top) * 0.5, at.z);
        mesh.beam(
            midpoint - along * jamb_width * 0.5,
            midpoint + along * jamb_width * 0.5,
            material.gate_post_depth(),
            top - at.y + 0.4,
            color,
        );
    }
    let left = Vec3::new(a.x, top, a.z) - along * (jamb_width + 0.1);
    let right = Vec3::new(b.x, top, b.z) + along * (jamb_width + 0.1);
    mesh.beam(left, right, thickness.max(0.9), 0.40, wood);
    let ridge = Vec3::Y * 1.0;
    let eave = outward * (thickness * 0.5 + 0.75);
    for side in [-1.0, 1.0] {
        // Overlapping solid courses form the two pitches. Beams include end
        // caps and undersides, so this roof reads correctly from ground level.
        for course in 0..4 {
            let t = course as f32 / 4.0;
            let offset = ridge * (1.0 - t) + eave * side * t;
            mesh.beam(
                left + offset,
                right + offset,
                eave.length() / 4.0 + 0.15,
                0.28,
                Vec3::new(0.42, 0.22, 0.13) * (0.94 + t * 0.12),
            );
        }
        mesh.beam(
            left + eave * side,
            right + eave * side,
            0.18,
            0.22,
            wood * 0.85,
        );
    }
    mesh.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn closed_beam_faces_point_outward() {
        let mut builder = MasonryMesh::default();
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

    #[test]
    fn walls_have_buried_bases_and_bounded_geometry() {
        for stone in [false, true] {
            let mesh = wall(
                Vec3::ZERO,
                Vec3::new(6.0, 0.3, 0.0),
                3.2,
                if stone { 1.2 } else { 0.44 },
                stone,
            );
            let Some(VertexAttributeValues::Float32x3(points)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            assert!(points
                .iter()
                .all(|p| p.iter().all(|value| value.is_finite())));
            assert!(points.iter().any(|p| p[1] < -0.1));
            assert!(
                points.len() < 6_000,
                "one six-metre section must remain inexpensive"
            );
        }
    }

    #[test]
    fn gateway_geometry_leaves_its_full_ground_passage_clear() {
        for stone in [false, true] {
            let mesh = gate(
                Vec3::new(-3.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
                if stone {
                    shared::components::FortificationMaterial::Stone
                } else {
                    shared::components::FortificationMaterial::Palisade
                },
            );
            let Some(VertexAttributeValues::Float32x3(points)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            assert!(points
                .iter()
                .filter(|p| p[0].abs() < 3.0)
                .all(|p| p[1] > 3.5));
        }
    }
}
