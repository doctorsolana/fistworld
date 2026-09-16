//! Terrain-following defense geometry.

use bevy::prelude::*;
use crate::settlement::structure_mesh::StructureMesh;

/// `a` and `b` are terrain-grade endpoints in mesh-local space. The shared
/// contract supplies height/thickness, keeping the silhouette and blocker in
/// agreement. The continuous timber backing closes cracks between faceted logs.
pub(super) fn wall(a: Vec3, b: Vec3, height: f32, thickness: f32, stone: bool) -> Mesh {
    let mut mesh = StructureMesh::default();
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
    let mut mesh = StructureMesh::default();
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
