//! Append-only defense reservations and region-scoped physical wall sections.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SettlementId;

/// Room for the eventual stone wall, its foundations and a narrow maintenance path.
pub const DEFENSE_CORRIDOR_HALF_WIDTH: f32 = 2.4;
pub const DEFENSE_GATE_MIN_WIDTH: f32 = 8.0;
/// Navigation type reserved for defenses, which remain solid even for the
/// hero's deliberate ordinary-building collision exemption.
pub const DEFENSE_OBSTACLE_TYPE: u32 = u32::MAX;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FortificationMaterial {
    Palisade,
    Stone,
}

impl FortificationMaterial {
    pub const fn height(self) -> f32 {
        match self {
            Self::Palisade => 3.2,
            Self::Stone => 4.6,
        }
    }
    pub const fn thickness(self) -> f32 {
        match self {
            Self::Palisade => 0.44,
            Self::Stone => 1.2,
        }
    }
    /// Clear height under a civic gateway. Gates remain open to ordinary traffic.
    pub const fn gate_clear_height(self) -> f32 {
        match self {
            Self::Palisade => 3.6,
            Self::Stone => 5.0,
        }
    }
    pub const fn good(self) -> crate::economy::Good {
        match self {
            Self::Palisade => crate::economy::Good::Wood,
            Self::Stone => crate::economy::Good::Stone,
        }
    }
    pub fn gate_post_width(self) -> f32 {
        self.thickness().max(0.45)
    }
    pub fn gate_post_depth(self) -> f32 {
        self.thickness().max(0.65)
    }
    /// Jambs sit outside the clear span, with the same tiny visual separation
    /// from neighboring wall ends used by the mesh generator.
    pub fn gate_post_offset(self) -> f32 {
        self.gate_post_width() * 0.5 + 0.015
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FortificationKind {
    Wall,
    Gate,
}

/// Geometry is immutable once accepted. A section completes only after paid
/// materials and actual civic labor; a later upgrade changes its material.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FortificationSegment {
    pub settlement_id: SettlementId,
    pub circuit: u8,
    pub start: Vec3,
    pub end: Vec3,
    pub kind: FortificationKind,
    pub material: FortificationMaterial,
    pub complete: bool,
}

impl FortificationSegment {
    pub fn length(&self) -> f32 {
        self.start.xz().distance(self.end.xz())
    }
    pub fn midpoint(&self) -> Vec3 {
        (self.start + self.end) * 0.5
    }
    pub fn material_required(&self, material: FortificationMaterial) -> u32 {
        let per_meter = match material {
            FortificationMaterial::Palisade => 0.6,
            FortificationMaterial::Stone => 1.0,
        };
        (self.length() * per_meter).ceil().max(1.0) as u32
            + if self.kind == FortificationKind::Gate {
                4
            } else {
                0
            }
    }
    /// Bevy yaw: local +X follows the section. All collision consumers share it.
    pub fn rotation(&self) -> f32 {
        let direction = self.end.xz() - self.start.xz();
        -direction.y.atan2(direction.x)
    }
    pub fn navigation_obstacle(&self) -> Option<crate::spatial::ObstacleEntry> {
        (self.complete && self.kind == FortificationKind::Wall).then(|| {
            crate::spatial::ObstacleEntry {
                center: self.midpoint().xz(),
                half_extents: Vec2::new(self.length() * 0.5, self.material.thickness() * 0.5)
                    + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                rotation: self.rotation(),
                obstacle_type: DEFENSE_OBSTACLE_TYPE,
            }
        })
    }
    pub fn gate_post_centers(&self) -> [Vec3; 2] {
        let axis = Vec3::new(self.end.x - self.start.x, 0.0, self.end.z - self.start.z)
            .normalize_or_zero();
        let offset = axis * self.material.gate_post_offset();
        [self.start - offset, self.end + offset]
    }
    /// Solid ground geometry, independently of adjacent sections' completion.
    /// The full gateway opening stays available; only its two jambs obstruct
    /// walking. Entries include the ordinary navigation capsule radius.
    pub fn ground_obstacles(&self) -> impl Iterator<Item = crate::spatial::ObstacleEntry> {
        let obstacles = if !self.complete {
            [None, None]
        } else if self.kind == FortificationKind::Wall {
            [self.navigation_obstacle(), None]
        } else {
            self.gate_post_centers().map(|center| {
                Some(crate::spatial::ObstacleEntry {
                    center: center.xz(),
                    half_extents: Vec2::new(
                        self.material.gate_post_width(),
                        self.material.gate_post_depth(),
                    ) * 0.5
                        + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                    rotation: self.rotation(),
                    obstacle_type: DEFENSE_OBSTACLE_TYPE,
                })
            })
        };
        obstacles.into_iter().flatten()
    }
    pub fn intersects_footprint(
        &self,
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
        clearance: f32,
    ) -> bool {
        oriented_rects_overlap(
            self.midpoint().xz(),
            Vec2::new(self.length() * 0.5, clearance),
            self.rotation(),
            center,
            half_extents,
            rotation,
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DefenseCircuit {
    pub id: u8,
    pub center: Vec2,
    pub boundary: Vec<Vec2>,
    pub sections: Vec<FortificationSegment>,
}

/// Hall-scoped immutable land reservation. It is separate from physical
/// sections so future building permits respect the corridor before work begins.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SettlementDefenses {
    pub circuits: Vec<DefenseCircuit>,
}

impl SettlementDefenses {
    pub fn intersects_footprint(&self, center: Vec2, half_extents: Vec2, rotation: f32) -> bool {
        self.circuits
            .iter()
            .flat_map(|c| &c.sections)
            .any(|section| {
                section.intersects_footprint(
                    center,
                    half_extents,
                    rotation,
                    DEFENSE_CORRIDOR_HALF_WIDTH,
                )
            })
    }
}

/// SAT in the same Bevy XZ rotation convention used by authored buildings and
/// SpatialObstacleGrid. Touching edges count as occupied land.
pub fn oriented_rects_overlap(a: Vec2, ah: Vec2, ar: f32, b: Vec2, bh: Vec2, br: f32) -> bool {
    let axes = |r: f32| [Vec2::new(r.cos(), -r.sin()), Vec2::new(r.sin(), r.cos())];
    let aa = axes(ar);
    let ba = axes(br);
    let distance = b - a;
    aa.into_iter().chain(ba).all(|axis| {
        let radius_a = ah.x * aa[0].dot(axis).abs() + ah.y * aa[1].dot(axis).abs();
        let radius_b = bh.x * ba[0].dot(axis).abs() + bh.y * ba[1].dot(axis).abs();
        distance.dot(axis).abs() <= radius_a + radius_b
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn section(kind: FortificationKind) -> FortificationSegment {
        FortificationSegment {
            settlement_id: SettlementId(42),
            circuit: 0,
            start: Vec3::new(-8., 0., 0.),
            end: Vec3::new(8., 0., 0.),
            kind,
            material: FortificationMaterial::Palisade,
            complete: true,
        }
    }
    #[test]
    fn a_completed_wall_blocks_crossing_but_an_open_gate_does_not() {
        let mut grid = crate::spatial::SpatialObstacleGrid::default();
        grid.insert(
            section(FortificationKind::Wall)
                .navigation_obstacle()
                .unwrap(),
        );
        assert!(grid.segment_blocked(Vec2::new(0., -5.), Vec2::new(0., 5.)));
        assert!(section(FortificationKind::Gate)
            .navigation_obstacle()
            .is_none());
        let mut pending = section(FortificationKind::Wall);
        pending.complete = false;
        assert!(pending.navigation_obstacle().is_none());
    }
    #[test]
    fn rotated_defense_reservations_match_navigation_yaw() {
        let mut wall = section(FortificationKind::Wall);
        wall.start = Vec3::new(-8., 0., -8.);
        wall.end = Vec3::new(8., 0., 8.);
        assert!(wall.intersects_footprint(Vec2::new(4., 4.), Vec2::splat(1.), 0., 2.4));
        assert!(!wall.intersects_footprint(Vec2::new(-5., 5.), Vec2::splat(1.), 0., 2.4));
        assert!(wall
            .navigation_obstacle()
            .unwrap()
            .contains_point(Vec2::new(4., 4.)));
    }
    #[test]
    fn defense_contract_roundtrips_exact_positions_and_material() {
        let wall = section(FortificationKind::Gate);
        let encoded = serde_json::to_vec(&wall).unwrap();
        assert_eq!(
            serde_json::from_slice::<FortificationSegment>(&encoded).unwrap(),
            wall
        );
    }
    #[test]
    fn standalone_gate_posts_block_before_neighboring_walls_but_preserve_the_opening() {
        for material in [
            FortificationMaterial::Palisade,
            FortificationMaterial::Stone,
        ] {
            for angle in [0.0_f32, 0.83] {
                let axis = Vec3::new(angle.cos(), 0.0, angle.sin());
                let normal = Vec2::new(-axis.z, axis.x);
                let mut gate = section(FortificationKind::Gate);
                gate.material = material;
                gate.start = -axis * 4.0;
                gate.end = axis * 4.0;
                let mut grid = crate::spatial::SpatialObstacleGrid::default();
                assert_eq!(gate.ground_obstacles().count(), 2);
                for obstacle in gate.ground_obstacles() {
                    grid.insert(obstacle);
                }
                assert!(!grid.segment_blocked(-normal * 5.0, normal * 5.0));
                for post in gate.gate_post_centers() {
                    assert!(grid.point_blocked(post.xz()));
                    assert!(
                        grid.segment_blocked(post.xz() - normal * 5.0, post.xz() + normal * 5.0)
                    );
                    assert!(
                        (post.distance(if post.dot(axis) < 0.0 {
                            gate.start
                        } else {
                            gate.end
                        }) - material.gate_post_offset())
                        .abs()
                            < 0.0001
                    );
                }
                gate.complete = false;
                assert_eq!(gate.ground_obstacles().count(), 0);
            }
        }
    }
}
