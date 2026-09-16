//! Shared fitting contract for PortDraft.glb (-Z seaward, authored deck Y=1).
use super::PortGeometry;
use bevy::prelude::*;

pub const PORT_PIER_WIDTH: f32 = 4.4;
pub const PORT_HEAD_WIDTH: f32 = 16.0;
pub const PORT_HEAD_DEPTH: f32 = 4.0;
pub const PORT_SHORE_WIDTH: f32 = 14.8;
pub const PORT_SHORE_DEPTH: f32 = 7.4;
pub const PORT_SHORE_FRONT: f32 = 2.0;
pub const PORT_AUTHORED_LENGTH: f32 = 20.0;
pub const PORT_BERTH_GAP: f32 = 0.75;
pub const PORT_OBSTACLE_TYPE: u32 = u32::MAX - 3;

/// Exact rectangles, not one convex collision hull around the walking space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortFootprint {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub yaw: f32,
}
impl PortFootprint {
    pub fn distance_squared(self, point: Vec2) -> f32 {
        let local = crate::rotation::world_to_local_xz(point - self.center, self.yaw);
        (local.abs() - self.half_extents)
            .max(Vec2::ZERO)
            .length_squared()
    }
    pub fn corners(self) -> [Vec2; 4] {
        let rotation = Quat::from_rotation_y(self.yaw);
        [
            Vec2::new(-1., -1.),
            Vec2::new(-1., 1.),
            Vec2::new(1., -1.),
            Vec2::new(1., 1.),
        ]
        .map(|sign| self.center + (rotation * (sign * self.half_extents).extend(0.).xzy()).xz())
    }
}
impl PortGeometry {
    pub fn length(self) -> f32 {
        self.shore.xz().distance(self.pier_end.xz())
    }
    pub fn seaward(self) -> Vec2 {
        (self.pier_end.xz() - self.shore.xz()).normalize_or_zero()
    }
    pub fn right(self) -> Vec2 {
        let sea = self.seaward();
        Vec2::new(-sea.y, sea.x)
    }
    /// Vessel yaw is separate: an alongside hull faces across the loading head.
    pub fn pier_yaw(self) -> f32 {
        let sea = self.seaward();
        (-sea.x).atan2(-sea.y)
    }
    pub fn deck_height(self, distance: f32) -> f32 {
        let t = ((distance - PORT_SHORE_FRONT)
            / (self.length() - PORT_HEAD_DEPTH - PORT_SHORE_FRONT))
            .clamp(0., 1.);
        self.shore.y + (self.pier_end.y - self.shore.y) * t
    }
    pub fn deck_point(self, distance: f32) -> Vec3 {
        let p = self.shore.xz() + self.seaward() * distance;
        Vec3::new(p.x, self.deck_height(distance), p.y)
    }
    /// Fixed shore/office and fixed loading head; only the middle corridor stretches.
    /// Returns world space so pile/foundation bottoms can sample authoritative ground.
    pub fn project_asset_point(self, local: Vec3) -> Vec3 {
        let authored = -local.z;
        let head_start = PORT_AUTHORED_LENGTH - PORT_HEAD_DEPTH;
        let distance = if authored <= PORT_SHORE_FRONT {
            authored
        } else if authored >= head_start {
            authored + self.length() - PORT_AUTHORED_LENGTH
        } else {
            PORT_SHORE_FRONT
                + (authored - PORT_SHORE_FRONT) / (head_start - PORT_SHORE_FRONT)
                    * (self.length() - PORT_HEAD_DEPTH - PORT_SHORE_FRONT)
        };
        let point = self.shore.xz() + self.seaward() * distance + self.right() * local.x;
        Vec3::new(point.x, self.deck_height(distance) + local.y - 1., point.y)
    }
    pub fn footprints(self) -> [PortFootprint; 3] {
        let head_start = self.length() - PORT_HEAD_DEPTH;
        let rectangle = |x, along, width, depth| PortFootprint {
            center: self.shore.xz() + self.right() * x + self.seaward() * along,
            half_extents: Vec2::new(width, depth) * 0.5,
            yaw: self.pier_yaw(),
        };
        [
            rectangle(
                -1.,
                PORT_SHORE_FRONT - PORT_SHORE_DEPTH * 0.5,
                PORT_SHORE_WIDTH,
                PORT_SHORE_DEPTH,
            ),
            rectangle(
                0.,
                (PORT_SHORE_FRONT + head_start) * 0.5,
                PORT_PIER_WIDTH,
                head_start - PORT_SHORE_FRONT,
            ),
            rectangle(
                0.,
                self.length() - PORT_HEAD_DEPTH * 0.5,
                PORT_HEAD_WIDTH,
                PORT_HEAD_DEPTH,
            ),
        ]
    }
    /// Height of the three joined walking surfaces, with pawn clearance from
    /// exposed edges. Office/cargo obstacles are checked by the caller's spatial
    /// index. The landward edge stays open to dry-ground entry, like a bridge ramp.
    pub fn walk_height_at(self, point: Vec2, clearance: f32) -> Option<f32> {
        if !self.valid() || !point.is_finite() || !clearance.is_finite() {
            return None;
        }
        let delta = point - self.shore.xz();
        let p = Vec2::new(delta.dot(self.right()), delta.dot(self.seaward()));
        let front = PORT_SHORE_FRONT;
        let back = front - PORT_SHORE_DEPTH;
        let left = -1. - PORT_SHORE_WIDTH * 0.5;
        let right = -1. + PORT_SHORE_WIDTH * 0.5;
        let pier = PORT_PIER_WIDTH * 0.5;
        let head = PORT_HEAD_WIDTH * 0.5;
        let end = self.length();
        let head_start = end - PORT_HEAD_DEPTH;
        let contains = |lo: Vec2, hi: Vec2| {
            p.cmpge(lo - Vec2::splat(0.001)).all() && p.cmple(hi + Vec2::splat(0.001)).all()
        };
        if !(contains(Vec2::new(left, back), Vec2::new(right, front))
            || contains(Vec2::new(-pier, front), Vec2::new(pier, head_start))
            || contains(Vec2::new(-head, head_start), Vec2::new(head, end)))
        {
            return None;
        }
        // Erode the UNION's exposed boundary, not each rectangle separately:
        // shrinking individual rectangles creates artificial gaps at both seams.
        let edges = [
            (Vec2::new(left, back), Vec2::new(left, front)),
            (Vec2::new(right, back), Vec2::new(right, front)),
            (Vec2::new(left, front), Vec2::new(-pier, front)),
            (Vec2::new(pier, front), Vec2::new(right, front)),
            (Vec2::new(-pier, front), Vec2::new(-pier, head_start)),
            (Vec2::new(pier, front), Vec2::new(pier, head_start)),
            (Vec2::new(-head, head_start), Vec2::new(-pier, head_start)),
            (Vec2::new(pier, head_start), Vec2::new(head, head_start)),
            (Vec2::new(-head, head_start), Vec2::new(-head, end)),
            (Vec2::new(head, head_start), Vec2::new(head, end)),
            (Vec2::new(-head, end), Vec2::new(head, end)),
        ];
        let radius_squared = clearance.max(0.).powi(2);
        edges
            .into_iter()
            .all(|(a, b)| {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0., 1.);
                p.distance_squared(a + ab * t) + 1e-6 >= radius_squared
            })
            .then(|| self.deck_height(p.y))
    }

    /// Whole port footprint, used for hull collision and before the pier exists.
    pub fn water_obstructs(self, point: Vec2, radius: f32) -> bool {
        self.footprints()
            .into_iter()
            .any(|rect| rect.distance_squared(point) <= radius * radius)
    }
    /// Solid authored objects only: never a convex hull around the walking
    /// deck. Coordinates are PortDraft's local X / seaward distance, with
    /// half-extents in metres. Small flat rope coils remain step-over dressing.
    pub fn ground_obstacles(self) -> impl Iterator<Item = crate::spatial::ObstacleEntry> {
        const SOLIDS: [(f32, f32, f32, f32); 19] = [
            (-5.25, -2.00, 2.50, 2.50), // office, including foundation/wall trim
            (4.38, -2.70, 1.06, 0.725), // loaded pallet
            (4.10, -0.35, 0.366, 0.366),
            (5.05, -0.70, 0.366, 0.366),
            (5.10, -1.45, 0.27, 0.27),
            (2.65, -3.85, 0.085, 0.085), // shelter posts; middle remains open
            (2.65, 0.25, 0.085, 0.085),
            (5.95, -3.85, 0.085, 0.085),
            (5.95, 0.25, 0.085, 0.085),
            (-1.98, 3.90, 0.10, 1.80), // approach rails, fitted with the walkway
            (1.98, 3.90, 0.10, 1.80),
            (-6., 19.65, 0.26, 0.225), // bollards
            (-3., 19.65, 0.26, 0.225),
            (3., 19.65, 0.26, 0.225),
            (6., 19.65, 0.26, 0.225),
            (-7.65, 17., 0.26, 0.225),
            (7.65, 17., 0.26, 0.225),
            (7.55, 18.4, 0.375, 0.935), // crane base and supported knee braces
            (-7.1, 18.95, 0.08, 0.08),  // lantern mast
        ];
        let valid = self.valid();
        SOLIDS
            .into_iter()
            .filter(move |_| valid)
            .map(move |(x, along, half_x, half_z)| {
                let first = self
                    .project_asset_point(Vec3::new(x, 1., -along + half_z))
                    .xz();
                let last = self
                    .project_asset_point(Vec3::new(x, 1., -along - half_z))
                    .xz();
                crate::spatial::ObstacleEntry {
                    center: (first + last) * 0.5,
                    half_extents: Vec2::new(half_x, first.distance(last) * 0.5)
                        + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                    rotation: self.pier_yaw(),
                    obstacle_type: PORT_OBSTACLE_TYPE,
                }
            })
    }
    pub fn valid(self) -> bool {
        self.shore.is_finite()
            && self.pier_end.is_finite()
            && self.berth.is_finite()
            && self.departure.is_finite()
            && self.yaw.is_finite()
            && (12.0..=80.0).contains(&self.length())
            && self.berth.xz().distance(self.pier_end.xz()) <= 24.0
            && (3.0..=48.0).contains(&self.berth.xz().distance(self.departure.xz()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::ShipKind;
    fn port() -> PortGeometry {
        PortGeometry {
            shore: Vec3::new(10., 2., 5.),
            pier_end: Vec3::new(10., 3., -29.),
            berth: Vec3::new(10., 2., -35.),
            departure: Vec3::new(25., 2., -35.),
            yaw: -std::f32::consts::FRAC_PI_2,
            maximum_ship: ShipKind::Cog,
        }
    }
    #[test]
    fn authored_port_projection_keeps_shore_and_head_sizes_and_matches_walk_surface() {
        let p = port();
        assert_eq!(p.project_asset_point(Vec3::new(0., 1., 0.)), p.shore);
        assert_eq!(p.project_asset_point(Vec3::new(0., 1., -20.)), p.pier_end);
        assert_eq!(
            p.project_asset_point(Vec3::new(0., 1., -2.)),
            p.deck_point(2.)
        );
        assert_eq!(
            p.project_asset_point(Vec3::new(0., 1., -16.)),
            p.deck_point(30.)
        );
        assert_eq!(
            p.project_asset_point(Vec3::new(8., 1., -20.)) - p.pier_end,
            Vec3::X * 8.
        );
        assert_eq!(p.deck_height(1.), 2.);
        assert_eq!(p.deck_height(32.), 3.);
        assert_eq!(p.footprints()[0].half_extents, Vec2::new(7.4, 3.7));
        assert_eq!(p.footprints()[2].half_extents, Vec2::new(8., 2.));
        assert!(p.water_obstructs(p.pier_end.xz() + Vec2::new(7., 1.), 0.));
        assert!(!p.water_obstructs(p.berth.xz(), 4.8));
    }
    #[test]
    fn rotated_footprints_and_ship_heading_do_not_rotate_the_shore_model() {
        let mut p = port();
        p.pier_end = p.shore + Vec3::X * 34.;
        p.pier_end.y = 3.;
        assert!((p.pier_yaw() + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        let head = p.footprints()[2];
        assert!(head.distance_squared(p.pier_end.xz() - Vec2::X) < 1e-6);
        for corner in head.corners() {
            assert!(head.distance_squared(corner) < 1e-8);
        }
    }
    #[test]
    fn walk_surface_joins_all_three_rectangles_without_clearance_gaps() {
        for rotated in [false, true] {
            let mut p = port();
            if rotated {
                let turn = -std::f32::consts::FRAC_PI_2;
                let rotation = Quat::from_rotation_y(turn);
                p.pier_end = p.shore + rotation * (p.pier_end - p.shore);
                p.berth = p.shore + rotation * (p.berth - p.shore);
                p.departure = p.shore + rotation * (p.departure - p.shore);
                p.yaw += turn;
            }
            assert!(
                p.valid(),
                "rotate the entire surveyed port, including its berth"
            );
            // Includes both exact joins and the dry-ground entrance edge.
            for along in [-5.4, 0., 1.8, 2., 2.2, 12., 29.8, 30., 30.2, 33.5] {
                assert!(
                    (p.walk_height_at(p.deck_point(along).xz(), 0.4).unwrap()
                        - p.deck_height(along))
                    .abs()
                        < 0.0001
                );
            }
            let near_edge = p.deck_point(32.).xz() + p.right() * 7.8;
            assert_eq!(p.walk_height_at(near_edge, 0.), Some(p.pier_end.y));
            assert_eq!(p.walk_height_at(near_edge, 0.4), None);
            assert_eq!(
                p.walk_height_at(p.deck_point(12.).xz() + p.right() * 3., 0.),
                None
            );
            assert_eq!(p.walk_height_at(p.deck_point(34.1).xz(), 0.), None);
            assert_eq!(p.walk_height_at(Vec2::splat(f32::NAN), 0.4), None);
        }
    }
}
