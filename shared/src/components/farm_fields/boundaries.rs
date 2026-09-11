//! One accepted boundary drives timber, land reservations and ground collision.

use super::FarmFieldShape;
use crate::components::{FarmField, VillageRoad};
use crate::rotation::{local_to_world_xz, world_to_local_xz};
use bevy::prelude::*;

pub const FARM_FENCE_HEIGHT: f32 = 0.95;
pub const FARM_FENCE_THICKNESS: f32 = 0.15;
pub const FARM_FENCE_OBSTACLE_TYPE: u32 = u32::MAX - 2;

impl FarmFieldShape {
    /// Centre of the front opening, in this worker area's local coordinates.
    /// An off-centre clipped parcel still receives a usable opening.
    pub fn entrance_local(&self, farm_offset: Vec2) -> Option<Vec2> {
        let first = self.sections.first()?;
        if !self.is_valid() {
            return None;
        }
        let center = (-farm_offset.x).clamp(first.left, first.right);
        let lo = first.left.max(center - 4.0);
        let hi = first.right.min(center + 4.0);
        Some(Vec2::new((lo + hi) * 0.5, first.z))
    }
    pub fn boundary_points(&self) -> Vec<Vec2> {
        if !self.is_valid() {
            return Vec::new();
        }
        self.sections
            .iter()
            .map(|s| Vec2::new(s.left, s.z))
            .chain(self.sections.iter().rev().map(|s| Vec2::new(s.right, s.z)))
            .collect()
    }

    /// Thin per-band reservation boxes. Conservative only between adjacent
    /// survey sections, never one large bounding box around an irregular farm.
    pub fn reservation_rects(&self, origin: Vec3, yaw: f32, margin: f32) -> Vec<(Vec3, Vec2, f32)> {
        if !self.is_valid() {
            return Vec::new();
        }
        self.sections
            .windows(2)
            .map(|s| {
                let min = Vec2::new(s[0].left.min(s[1].left), s[0].z);
                let max = Vec2::new(s[0].right.max(s[1].right), s[1].z);
                let center = origin.xz() + local_to_world_xz((min + max) * 0.5, yaw);
                (
                    Vec3::new(center.x, origin.y, center.y),
                    (max - min) * 0.5 + Vec2::splat(margin),
                    yaw,
                )
            })
            .collect()
    }

    pub fn intersects_road(&self, origin: Vec3, yaw: f32, road: &VillageRoad, margin: f32) -> bool {
        let boundary = self.boundary_points();
        if boundary.is_empty() {
            return false;
        }
        let radius = road.reservation_width() * 0.5 + margin;
        road.points.windows(2).any(|segment| {
            let a = world_to_local_xz(segment[0] - origin.xz(), yaw);
            let b = world_to_local_xz(segment[1] - origin.xz(), yaw);
            self.contains_local_point(a, radius)
                || self.contains_local_point(b, radius)
                || boundary
                    .iter()
                    .zip(boundary.iter().cycle().skip(1))
                    .any(|(&c, &d)| {
                        segments_intersect(a, b, c, d)
                            || point_segment_distance(a, c, d) <= radius
                            || point_segment_distance(b, c, d) <= radius
                            || point_segment_distance(c, a, b) <= radius
                            || point_segment_distance(d, a, b) <= radius
                    })
        })
    }
}

impl FarmField {
    pub fn accepted_shape(&self) -> FarmFieldShape {
        self.shape
            .clone()
            .unwrap_or_else(FarmFieldShape::legacy_rectangle)
    }

    /// World-space crop reservation bands used by conservative site planning.
    /// The final field and tree-clearing tests continue to use the exact shape.
    pub fn reservation_rects(&self, origin: Vec3, yaw: f32, margin: f32) -> Vec<(Vec3, Vec2, f32)> {
        self.accepted_shape().reservation_rects(origin, yaw, margin)
    }

    /// Local boundary segments with a broad front entrance and no fence on the
    /// artificial worker split at farm X=0. Both halves remain one shared field.
    pub fn fence_segments(&self, origin: Vec3, yaw: f32) -> Vec<(Vec2, Vec2)> {
        if self.layout_version < 2 {
            return Vec::new();
        }
        let shape = self.accepted_shape();
        let boundary = shape.boundary_points();
        let Some(first) = shape.sections.first() else {
            return Vec::new();
        };
        let offset = world_to_local_xz(origin.xz() - self.farmstead.xz(), yaw);
        let center = (-offset.x).clamp(first.left, first.right);
        let gate = (center - 4.0, center + 4.0);
        let mut result = Vec::new();
        for (&a, &b) in boundary.iter().zip(boundary.iter().cycle().skip(1)) {
            // Legacy roots leave a0.45m verge on either side of farm X=0.
            // Those inward edges are still the worker split, never a fence.
            if (a.x + offset.x).abs() <= 0.55 && (b.x + offset.x).abs() <= 0.55 {
                continue;
            }
            if (a.y - first.z).abs() < 0.01 && (b.y - first.z).abs() < 0.01 {
                let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
                for (start, end) in [(lo, hi.min(gate.0)), (lo.max(gate.1), hi)] {
                    if end - start > 0.2 {
                        result.push((Vec2::new(start, a.y), Vec2::new(end, a.y)));
                    }
                }
            } else if a.distance_squared(b) > 0.04 {
                result.push((a, b));
            }
        }
        result
    }

    pub fn ground_obstacles(&self, origin: Vec3, yaw: f32) -> Vec<crate::spatial::ObstacleEntry> {
        self.fence_segments(origin, yaw)
            .into_iter()
            .map(|(a, b)| {
                let a = origin.xz() + local_to_world_xz(a, yaw);
                let b = origin.xz() + local_to_world_xz(b, yaw);
                let d = b - a;
                crate::spatial::ObstacleEntry {
                    center: (a + b) * 0.5,
                    half_extents: Vec2::new(d.length() * 0.5, FARM_FENCE_THICKNESS * 0.5)
                        + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                    rotation: (-d.y).atan2(d.x),
                    obstacle_type: FARM_FENCE_OBSTACLE_TYPE,
                }
            })
            .collect()
    }
}

fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    p.distance(a + d * ((p - a).dot(d) / d.length_squared().max(1e-8)).clamp(0., 1.))
}
fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let u = b - a;
    let v = d - c;
    let denominator = u.perp_dot(v);
    if denominator.abs() < 1e-8 {
        return false;
    }
    let t = (c - a).perp_dot(v) / denominator;
    let s = (c - a).perp_dot(u) / denominator;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_worker_split_and_broad_front_gate_remain_open() {
        let shapes = crate::components::fit_farm_field_shapes(Vec3::ZERO, 0., 5, |_| true);
        for (i, shape) in shapes.into_iter().enumerate() {
            let origin = crate::components::SettlementBuildingKind::Farmstead
                .field_position_at(Vec3::ZERO, 0., i as u8)
                .unwrap();
            let field = FarmField {
                settlement: "Test".into(),
                farmstead: Vec3::ZERO,
                plot_index: i as u8,
                quality: 1.,
                shape: Some(shape),
                layout_version: 2,
            };
            let segments = field.fence_segments(origin, 0.);
            assert!(!segments.is_empty());
            for (a, b) in segments {
                assert!(!((a.x + origin.x).abs() < 0.03 && (b.x + origin.x).abs() < 0.03));
                let z = field.shape.as_ref().unwrap().sections[0].z;
                if (a.y - z).abs() < 0.01 && (b.y - z).abs() < 0.01 {
                    assert!(
                        (a.x + origin.x).max(b.x + origin.x) <= -4.0
                            || (a.x + origin.x).min(b.x + origin.x) >= 4.0
                    );
                }
            }
            assert_eq!(
                field.ground_obstacles(origin, 0.).len(),
                field.fence_segments(origin, 0.).len()
            );
            assert_eq!(field.productive_fraction(), 1.0);
        }
    }
}
