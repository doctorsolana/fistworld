//! Accepted household outdoor space. The server owns the small land decision;
//! clients derive its dressing and collision uses the same boundary segments.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{HouseAppearance, SettlementBuildingKind};
use crate::rotation::{local_to_world_xz, world_to_local_xz};
use crate::spatial::ObstacleEntry;

mod geometry;
mod land;
use geometry::{area, bounds, centroid, contains};
pub use land::HouseholdYardLand;

pub const YARD_FENCE_HEIGHT: f32 = 0.92;
pub const YARD_FENCE_THICKNESS: f32 = 0.13;
pub const YARD_OBSTACLE_TYPE: u32 = u32::MAX - 1;
// This is a planting setback, not a navigation corridor. Access uses the
// entire unfenced streetward end; do not close that end with a return fence.
const YARD_HOUSE_GAP: f32 = 0.55;

pub fn household_yard_seed(origin: Vec3) -> u64 {
    let x = (origin.x * 4.).round() as i64 as u64;
    let z = (origin.z * 4.).round() as i64 as u64;
    crate::worldgen::splitmix64(x ^ z.rotate_left(32) ^ 0x5941_5244_5F48_4F4D)
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum YardSide {
    Left,
    Right,
    Rear,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum YardUse {
    Vegetables,
    Laundry,
    Flowers,
    Firewood,
}

/// Local X/Z bounds in the owning house's frame. The house-facing side remains
/// entirely open: this is a working yard, not a decorative cage around a door.
/// No ownership or food production is implied by ornamental household plants.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HouseholdYard {
    pub minimum: Vec2,
    pub maximum: Vec2,
    pub side: YardSide,
    pub use_kind: YardUse,
    pub seed: u64,
    /// Counter-clockwise local X/Z parcel boundary. Old snapshots omit this
    /// and retain their rectangular bounds; new authoritative fits clip it
    /// against surrounding land. The owning house remains the stable owner.
    #[serde(default)]
    pub boundary: Vec<Vec2>,
}

impl HouseholdYard {
    pub fn center(&self) -> Vec2 {
        if self.boundary.is_empty() {
            (self.minimum + self.maximum) * 0.5
        } else {
            centroid(&self.boundary)
        }
    }

    pub fn boundary_points(&self) -> Vec<Vec2> {
        if self.boundary.is_empty() {
            vec![
                self.minimum,
                Vec2::new(self.maximum.x, self.minimum.y),
                self.maximum,
                Vec2::new(self.minimum.x, self.maximum.y),
            ]
        } else {
            self.boundary.clone()
        }
    }

    pub fn contains_local_point(&self, point: Vec2, margin: f32) -> bool {
        if self.boundary.is_empty() {
            point.cmpge(self.minimum - Vec2::splat(margin)).all()
                && point.cmple(self.maximum + Vec2::splat(margin)).all()
        } else {
            contains(&self.boundary, point, margin)
        }
    }

    pub fn area(&self) -> f32 {
        if self.boundary.is_empty() {
            (self.maximum.x - self.minimum.x) * (self.maximum.y - self.minimum.y)
        } else {
            area(&self.boundary)
        }
    }

    pub fn world_bounds(&self, origin: Vec3, yaw: f32, margin: f32) -> (Vec2, Vec2) {
        let bounds = crate::spatial::ObstacleAABB::from_center_extents(
            origin.xz() + local_to_world_xz((self.minimum + self.maximum) * 0.5, yaw),
            (self.maximum - self.minimum) * 0.5 + Vec2::splat(margin),
            yaw,
        );
        (bounds.min, bounds.max)
    }

    /// Terrain edits in another town must not rebuild this garden. Chunk
    /// revisions include boundary-sampling neighbours; replacement invalidates all.
    pub fn terrain_signature(
        &self,
        origin: Vec3,
        yaw: f32,
        terrain: &crate::terrain::WorldTerrain,
    ) -> u64 {
        let (min, max) = self.world_bounds(origin, yaw, 0.50);
        let lo = crate::terrain::ChunkCoord::from_world_pos(Vec3::new(min.x, 0., min.y));
        let hi = crate::terrain::ChunkCoord::from_world_pos(Vec3::new(max.x, 0., max.y));
        let mut signature = u64::from(terrain.full_rebuild_version());
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                signature = crate::worldgen::splitmix64(
                    signature
                        ^ u64::from(
                            terrain
                                .chunk_modification_version(crate::terrain::ChunkCoord::new(x, z)),
                        ),
                );
            }
        }
        signature
    }

    pub fn fits_site(
        &self,
        origin: Vec3,
        yaw: f32,
        mut clear: impl FnMut(Vec2, f32) -> bool,
        mut ground: impl FnMut(Vec2) -> Option<f32>,
    ) -> bool {
        let size = self.maximum - self.minimum;
        if !size.is_finite() || size.min_element() < 1.0 || size.max_element() > 9.0 {
            return false;
        }
        let house = SettlementBuildingKind::House.placement_definition();
        let lo = house.footprint_center - house.footprint * 0.5;
        let hi = house.footprint_center + house.footprint * 0.5;
        let entrance_gap = match self.side {
            YardSide::Left => lo.x - self.maximum.x,
            YardSide::Right => self.minimum.x - hi.x,
            YardSide::Rear => self.minimum.y - hi.y,
        };
        // Reject externally supplied bounds that encroach on the future home.
        if !entrance_gap.is_finite() || entrance_gap < YARD_HOUSE_GAP - 0.001 {
            return false;
        }
        if !self.boundary.is_empty() {
            if !geometry::valid_convex(&self.boundary) || self.area() < 4.5 {
                return false;
            }
            let (minimum, maximum) = bounds(&self.boundary);
            if minimum.distance_squared(self.minimum) > 0.0001
                || maximum.distance_squared(self.maximum) > 0.0001
            {
                return false;
            }
        }
        let steps = (size / 0.45).ceil().as_uvec2();
        let mut min_height = f32::INFINITY;
        let mut max_height = f32::NEG_INFINITY;
        for z in 0..=steps.y {
            for x in 0..=steps.x {
                let local = self.minimum
                    + size * Vec2::new(x as f32 / steps.x as f32, z as f32 / steps.y as f32);
                if !self.contains_local_point(local, 0.001) {
                    continue;
                }
                let world = origin.xz() + local_to_world_xz(local, yaw);
                let Some(height) = ground(world).filter(|h| h.is_finite()) else {
                    return false;
                };
                if !clear(world, 0.40) {
                    return false;
                }
                min_height = min_height.min(height);
                max_height = max_height.max(height);
            }
        }
        // Grid samples alone can miss a clipped corner. Validate the exact
        // boundary too, where all solid fence geometry will be authored.
        let boundary = self.boundary_points();
        for i in 0..boundary.len() {
            let a = boundary[i];
            let b = boundary[(i + 1) % boundary.len()];
            let n = (a.distance(b) / 0.4).ceil().max(1.0) as usize;
            for j in 0..=n {
                let world = origin.xz() + local_to_world_xz(a.lerp(b, j as f32 / n as f32), yaw);
                let Some(height) = ground(world).filter(|h| h.is_finite()) else {
                    return false;
                };
                if !clear(world, 0.40) {
                    return false;
                }
                min_height = min_height.min(height);
                max_height = max_height.max(height);
            }
        }
        max_height - min_height <= 0.85
    }

    pub fn contains_world_point(&self, point: Vec2, origin: Vec3, yaw: f32, margin: f32) -> bool {
        let p = world_to_local_xz(point - origin.xz(), yaw);
        self.contains_local_point(p, margin)
    }

    /// Two joined boundaries leave a broad open corner toward the street.
    /// Closing the third side would turn the narrow planting setback beside
    /// an upgraded house into the only escape: visually open, but too tight
    /// for embodied agents and the local A* lattice. Both client LODs and
    /// navigation derive these same spans, including existing saved yards.
    pub fn fence_segments(&self) -> Vec<(Vec2, Vec2)> {
        let boundary = self.boundary_points();
        boundary
            .iter()
            .copied()
            .zip(boundary.iter().copied().cycle().skip(1))
            .take(boundary.len())
            .filter(|(a, b)| {
                let edge = *b - *a;
                let outward = Vec2::new(edge.y, -edge.x).normalize_or_zero();
                // Open the homeward edge AND a full streetward corner. A
                // clipped road-facing side still gets a fence when its normal
                // points away from the home, so it follows the real parcel.
                match self.side {
                    YardSide::Right => outward.x > 0.35 || outward.y > 0.80,
                    YardSide::Left => outward.x < -0.35 || outward.y > 0.80,
                    YardSide::Rear => outward.y > 0.35 || outward.x < -0.80,
                }
            })
            .collect()
    }

    /// Longest boundary facing away from the home. Laundry supports and the
    /// wood rack use this actual clipped edge, never an old bounding-box chord.
    pub fn outer_edge(&self) -> (Vec2, Vec2) {
        let direction = match self.side {
            YardSide::Left => Vec2::NEG_X,
            YardSide::Right => Vec2::X,
            YardSide::Rear => Vec2::Y,
        };
        self.fence_segments()
            .into_iter()
            .max_by(|(a, b), (c, d)| {
                let score = |a: Vec2, b: Vec2| {
                    let e = b - a;
                    let n = Vec2::new(e.y, -e.x).normalize_or_zero();
                    a.distance(b) * (0.25 + n.dot(direction).max(0.0))
                };
                score(*a, *b).total_cmp(&score(*c, *d))
            })
            .unwrap_or((self.minimum, self.maximum))
    }

    pub fn ground_obstacles(&self, origin: Vec3, yaw: f32) -> Vec<ObstacleEntry> {
        let mut result = Vec::with_capacity(4);
        for (a, b) in self.fence_segments() {
            let a = origin.xz() + local_to_world_xz(a, yaw);
            let b = origin.xz() + local_to_world_xz(b, yaw);
            let delta = b - a;
            result.push(ObstacleEntry {
                center: (a + b) * 0.5,
                half_extents: Vec2::new(delta.length() * 0.5, YARD_FENCE_THICKNESS * 0.5)
                    + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                rotation: (-delta.y).atan2(delta.x),
                obstacle_type: YARD_OBSTACLE_TYPE,
            });
        }
        if let Some((center, tangent)) = self.firewood_frame() {
            result.push(ObstacleEntry {
                center: origin.xz() + local_to_world_xz(center, yaw),
                half_extents: Vec2::new(1.05, 0.42)
                    + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                rotation: yaw + (-tangent.y).atan2(tangent.x),
                obstacle_type: YARD_OBSTACLE_TYPE,
            });
        }
        result
    }

    /// Shared supported-rack placement. Logs need both their own footprint and
    /// a useful working apron. Merely fitting the wood can leave a narrow slot
    /// between its padded collider and the upgraded house with no route-grid
    /// connection. Small/clipped yards omit the stack in both visual LODs and
    /// collision instead of trapping their residents.
    pub fn firewood_frame(&self) -> Option<(Vec2, Vec2)> {
        if self.use_kind != YardUse::Firewood {
            return None;
        }
        let (a, b) = self.outer_edge();
        if a.distance(b) < 2.3 {
            return None;
        }
        let tangent = (b - a).normalize();
        let inward = Vec2::new(-tangent.y, tangent.x);
        let center = (a + b) * 0.5 + inward * 0.52;
        // The 0.55 m gap outside this parcel is only a planting setback.
        // Reserve the apron inside our accepted land instead of relying on
        // unclaimed space behind the house or across an angled road boundary.
        const WORK_APRON_DEPTH: f32 = 1.6;
        [-1., 1.]
            .into_iter()
            .all(|x| {
                [-0.42, 0.42 + WORK_APRON_DEPTH].into_iter().all(|depth| {
                    self.contains_local_point(center + tangent * (x * 1.05) + inward * depth, -0.05)
                })
            })
            .then_some((center, tangent))
    }
}

/// Site-fit a modest side/rear yard beyond the *future* house envelope. The
/// caller supplies exact land/road/field clearance and terrain/water sampling.
/// Work is bounded (three sides, three sizes, a sub-metre sample grid). The
/// same function stages old offline snapshots; live clients never grant land.
pub fn fit_household_yard(
    _appearance: HouseAppearance,
    origin: Vec3,
    yaw: f32,
    seed: u64,
    mut clear: impl FnMut(Vec2, f32) -> bool,
    mut ground: impl FnMut(Vec2) -> Option<f32>,
) -> Option<HouseholdYard> {
    let definition = SettlementBuildingKind::House.placement_definition();
    let lo = definition.footprint_center - definition.footprint * 0.5;
    let hi = definition.footprint_center + definition.footprint * 0.5;
    let sides = if seed & 1 == 0 {
        [YardSide::Right, YardSide::Rear, YardSide::Left]
    } else {
        [YardSide::Left, YardSide::Rear, YardSide::Right]
    };
    for side in sides {
        for (depth, length) in [(2.8, 5.6), (2.2, 4.8), (1.6, 3.6)] {
            // Keep the planting close to the home. Actual access is through
            // the wide unfenced corner, not this narrow planting setback.
            let (minimum, maximum) = match side {
                YardSide::Right => (
                    Vec2::new(hi.x + YARD_HOUSE_GAP, -1.8),
                    Vec2::new(hi.x + YARD_HOUSE_GAP + depth, -1.8 + length),
                ),
                YardSide::Left => (
                    Vec2::new(lo.x - YARD_HOUSE_GAP - depth, -1.8),
                    Vec2::new(lo.x - YARD_HOUSE_GAP, -1.8 + length),
                ),
                YardSide::Rear => (
                    Vec2::new(-length * 0.5, hi.y + YARD_HOUSE_GAP),
                    Vec2::new(length * 0.5, hi.y + YARD_HOUSE_GAP + depth),
                ),
            };
            let yard = HouseholdYard {
                minimum,
                maximum,
                side,
                seed,
                boundary: Vec::new(),
                use_kind: match (seed >> 8) % 5 {
                    0 | 1 => YardUse::Vegetables,
                    2 => YardUse::Laundry,
                    3 => YardUse::Flowers,
                    _ => YardUse::Firewood,
                },
            };
            if yard.fits_site(origin, yaw, &mut clear, &mut ground) {
                return Some(yard);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firewood_needs_a_full_working_apron_within_its_accepted_land() {
        let definition = SettlementBuildingKind::House.placement_definition();
        let lo = definition.footprint_center - definition.footprint * 0.5;
        let hi = definition.footprint_center + definition.footprint * 0.5;
        for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
            for (depth, has_rack) in [(1.6, false), (2.2, false), (2.8, true)] {
                let yard = fit_household_yard(
                    HouseAppearance::default(),
                    Vec3::ZERO,
                    0.,
                    4 << 8,
                    |p, radius| {
                        let distance = match side {
                            YardSide::Left => lo.x - p.x,
                            YardSide::Right => p.x - hi.x,
                            YardSide::Rear => p.y - hi.y,
                        };
                        distance > 0. && distance + radius < YARD_HOUSE_GAP + depth + 0.41
                    },
                    |_| Some(0.),
                )
                .unwrap();
                assert_eq!(yard.side, side);
                assert_eq!(
                    yard.firewood_frame().is_some(),
                    has_rack,
                    "{side:?}, depth {depth}"
                );
                assert_eq!(
                    yard.ground_obstacles(Vec3::ZERO, 0.).len(),
                    yard.fence_segments().len() + usize::from(has_rack)
                );
            }
        }
    }

    #[test]
    fn asymmetric_parcel_bounds_include_every_rotated_corner() {
        let yard = HouseholdYard {
            minimum: Vec2::new(5., 0.),
            maximum: Vec2::new(9., 5.),
            side: YardSide::Right,
            use_kind: YardUse::Vegetables,
            seed: 0,
            boundary: vec![
                Vec2::new(5., 0.),
                Vec2::new(9., 0.),
                Vec2::new(6., 5.),
                Vec2::new(5., 5.),
            ],
        };
        let origin = Vec3::new(61.5, 2., -63.7);
        for step in 0..16 {
            let yaw = step as f32 * std::f32::consts::TAU / 16.;
            let (lo, hi) = yard.world_bounds(origin, yaw, 0.);
            for corner in yard.boundary_points() {
                let p = origin.xz() + local_to_world_xz(corner, yaw);
                assert!(p.cmpge(lo - Vec2::splat(0.0001)).all());
                assert!(p.cmple(hi + Vec2::splat(0.0001)).all());
            }
        }
    }

    #[test]
    fn every_yard_has_a_real_exit_around_the_combined_upgraded_house_and_fences() {
        let reserved = SettlementBuildingKind::House.placement_definition();
        let lo = reserved.footprint_center - reserved.footprint * 0.5;
        let hi = reserved.footprint_center + reserved.footprint * 0.5;
        for side in [YardSide::Left, YardSide::Right, YardSide::Rear] {
            for art in [
                crate::building::BuildingType::LogCabin,
                crate::building::BuildingType::LongCabin,
                crate::building::BuildingType::CabinL2,
                crate::building::BuildingType::LongCabinL2,
            ] {
                for step in 0..16 {
                    let yaw = step as f32 * std::f32::consts::TAU / 16.0;
                    let origin = Vec3::new(37.3, 2.0, -21.7);
                    let yard = fit_household_yard(
                        HouseAppearance::default(),
                        origin,
                        yaw,
                        0,
                        |point, _| {
                            let p = world_to_local_xz(point - origin.xz(), yaw);
                            match side {
                                YardSide::Left => p.x < lo.x,
                                YardSide::Right => p.x > hi.x,
                                YardSide::Rear => p.y > hi.y,
                            }
                        },
                        |_| Some(2.0),
                    )
                    .unwrap();
                    assert_eq!(yard.side, side);
                    let definition = art.definition();
                    let mut grid = crate::spatial::SpatialObstacleGrid::default();
                    grid.insert(ObstacleEntry {
                        center: definition.world_footprint_center(origin, yaw),
                        half_extents: definition.footprint * 0.5
                            + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                        rotation: yaw,
                        obstacle_type: art as u32,
                    });
                    for obstacle in yard.ground_obstacles(origin, yaw) {
                        grid.insert(obstacle);
                    }
                    let outside = match side {
                        YardSide::Left | YardSide::Right => {
                            Vec2::new(yard.center().x, lo.y.min(yard.minimum.y) - 1.0)
                        }
                        YardSide::Rear => {
                            Vec2::new(hi.x.max(yard.maximum.x) + 1.0, yard.center().y)
                        }
                    };
                    let path =
                        [yard.center(), outside].map(|p| origin.xz() + local_to_world_xz(p, yaw));
                    for segment in path.windows(2) {
                        assert!(
                            !grid.segment_blocked(segment[0], segment[1]),
                            "{art:?}/{side:?} yaw {yaw}: apparent opening is blocked"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn yards_leave_upgrade_envelope_and_actual_door_corridor_clear() {
        for step in 0..16 {
            let yaw = step as f32 * std::f32::consts::TAU / 16.0;
            let origin = Vec3::new(37.0, 2.0, -21.0);
            let yard = fit_household_yard(
                HouseAppearance::default(),
                origin,
                yaw,
                step,
                |_, _| true,
                |_| Some(2.0),
            )
            .unwrap();
            let door = SettlementBuildingKind::House.entrance_position(origin, yaw);
            assert!(!yard.contains_world_point(door.xz(), origin, yaw, 0.4));
            let mut obstacles = crate::spatial::SpatialObstacleGrid::default();
            for obstacle in yard.ground_obstacles(origin, yaw) {
                obstacles.insert(obstacle);
            }
            assert!(!obstacles.segment_blocked(
                door.xz(),
                door.xz() + local_to_world_xz(Vec2::new(0.0, -4.0), yaw)
            ));
            let center = origin.xz() + local_to_world_xz(yard.center(), yaw);
            let opening = match yard.side {
                YardSide::Left => Vec2::new(yard.maximum.x + 0.5, yard.center().y),
                YardSide::Right => Vec2::new(yard.minimum.x - 0.5, yard.center().y),
                YardSide::Rear => Vec2::new(yard.center().x, yard.minimum.y - 0.5),
            };
            assert!(
                !obstacles.segment_blocked(origin.xz() + local_to_world_xz(opening, yaw), center)
            );
        }
    }

    #[test]
    fn constrained_land_shrinks_or_omits_yards_instead_of_crossing_a_road() {
        let fit = |limit| {
            fit_household_yard(
                HouseAppearance::default(),
                Vec3::ZERO,
                0.,
                0,
                |p, r| p.x - r > 4.4 && p.x + r < limit,
                |_| Some(0.),
            )
        };
        let yard = fit(7.7).unwrap();
        assert!(yard.maximum.x < 7.7);
        assert!(yard.maximum.x - yard.minimum.x < 2.8);
        assert!(fit(5.5).is_none());
        assert!(fit_household_yard(
            HouseAppearance::default(),
            Vec3::ZERO,
            0.,
            0,
            |_, _| true,
            |_| None
        )
        .is_none());
        assert!(fit_household_yard(
            HouseAppearance::default(),
            Vec3::ZERO,
            0.,
            0,
            |_, _| true,
            |p| Some(p.x * 2. + p.y * 2.)
        )
        .is_none());
    }

    #[test]
    fn accepted_yard_wire_roundtrip_keeps_real_fence_and_seed() {
        let yard = fit_household_yard(
            HouseAppearance::default(),
            Vec3::ZERO,
            0.,
            273,
            |_, _| true,
            |_| Some(0.),
        )
        .unwrap();
        let bytes = bincode::serialize(&yard).unwrap();
        let decoded: HouseholdYard = bincode::deserialize(&bytes).unwrap();
        assert_eq!(yard, decoded);
        assert_eq!(yard.fence_segments(), decoded.fence_segments());
    }

    #[test]
    fn validation_signatures_ignore_remote_edits_but_detect_nearby_land_and_terrain() {
        let origin = Vec3::new(30., 0., -15.);
        let yaw = 0.4;
        let yard = fit_household_yard(
            HouseAppearance::default(),
            origin,
            yaw,
            4,
            |_, _| true,
            |_| Some(0.),
        )
        .unwrap();
        let mut land = HouseholdYardLand::default();
        land.reserve_building(SettlementBuildingKind::House, origin, yaw);
        let local = land.signature_for_yard(&yard, origin, yaw);
        land.reserve_building(
            SettlementBuildingKind::Farmstead,
            Vec3::new(1000., 0., 1000.),
            0.,
        );
        assert_eq!(land.signature_for_yard(&yard, origin, yaw), local);
        land.reserve_rect(
            origin.xz() + local_to_world_xz(yard.center(), yaw),
            Vec2::splat(0.5),
            yaw,
        );
        assert_ne!(land.signature_for_yard(&yard, origin, yaw), local);

        let mut terrain = crate::terrain::WorldTerrain::default();
        let revision = yard.terrain_signature(origin, yaw, &terrain);
        terrain.apply_flatten_rect(Vec3::new(1000., 80., 1000.), Vec2::splat(20.), 0., 2.);
        assert_eq!(yard.terrain_signature(origin, yaw, &terrain), revision);
        terrain.apply_flatten_rect(origin + Vec3::Y * 80., Vec2::splat(20.), 0., 2.);
        assert_ne!(yard.terrain_signature(origin, yaw, &terrain), revision);
    }
}
