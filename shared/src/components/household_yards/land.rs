//! Deterministic nearby land clipping, shared by live authoring and art fixtures.

use super::{geometry, HouseholdYard, YardSide};
use crate::components::{
    CivicHallLevel, HouseAppearance, SettlementBuildingKind, VillageRoad, YardUse,
};
use crate::rotation::{local_to_world_xz, world_to_local_xz};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

struct Reservation {
    points: Vec<Vec2>,
    margin: f32,
    signature: u64,
}

/// A yard belongs to its existing house, never to a second independently
/// spawned property entity. These local reservations only decide revocable
/// outdoor space; roads, actual buildings and known upgrades take priority.
#[derive(Default)]
pub struct HouseholdYardLand {
    reservations: Vec<Reservation>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    source_cells: HashMap<(i32, i32), u64>,
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / 16.).floor() as i32, (p.y / 16.).floor() as i32)
}

impl HouseholdYardLand {
    fn reserve_polygon(&mut self, points: Vec<Vec2>, margin: f32) {
        let mut signature = u64::from(margin.to_bits());
        for p in &points {
            for f in p.to_array() {
                signature = crate::worldgen::splitmix64(signature ^ u64::from(f.to_bits()));
            }
        }
        let (lo, hi) = geometry::bounds(&points);
        let a = cell(lo - Vec2::splat(margin));
        let b = cell(hi + Vec2::splat(margin));
        let index = self.reservations.len();
        for x in a.0..=b.0 {
            for z in a.1..=b.1 {
                self.cells.entry((x, z)).or_default().push(index);
                let entry = self.source_cells.entry((x, z)).or_default();
                *entry = entry.wrapping_add(signature);
            }
        }
        self.reservations.push(Reservation {
            points,
            margin,
            signature,
        });
    }

    pub fn reserve_rect(&mut self, center: Vec2, half: Vec2, yaw: f32) {
        self.reserve_polygon(
            [
                Vec2::new(-half.x, -half.y),
                Vec2::new(half.x, -half.y),
                half,
                Vec2::new(-half.x, half.y),
            ]
            .map(|p| center + local_to_world_xz(p, yaw))
            .to_vec(),
            0.0,
        );
    }

    pub fn reserve_segment(&mut self, a: Vec2, b: Vec2, half_width: f32) {
        let delta = b - a;
        self.reserve_rect(
            (a + b) * 0.5,
            Vec2::new(delta.length() * 0.5 + half_width, half_width),
            (-delta.y).atan2(delta.x),
        );
    }

    pub fn reserve_building(&mut self, kind: SettlementBuildingKind, origin: Vec3, yaw: f32) {
        self.reserve_building_without_inferred_fields(kind, origin, yaw);
        if let (Some(fields), Some(half)) =
            (kind.field_positions(origin, yaw), kind.field_half_extents())
        {
            for field in fields {
                self.reserve_rect(
                    field.xz(),
                    half + Vec2::splat(crate::components::FARM_FIELD_TERRACE_MARGIN),
                    yaw,
                );
            }
        }
    }

    /// Pending farms reserve their proposed large envelope. Completed farms
    /// retain accepted sections (or legacy inferred rectangles), so granting a
    /// new farm cannot retroactively expand an old neighbour's land claim.
    pub fn reserve_new_building(&mut self, kind: SettlementBuildingKind, origin: Vec3, yaw: f32) {
        self.reserve_building_without_inferred_fields(kind, origin, yaw);
        if let (Some(fields), Some(half)) = (
            kind.intended_field_positions(origin, yaw),
            kind.intended_field_half_extents(),
        ) {
            for field in fields {
                self.reserve_rect(
                    field.xz(),
                    half + Vec2::splat(crate::components::FARM_FIELD_TERRACE_MARGIN),
                    yaw,
                );
            }
        }
    }

    pub fn reserve_building_without_inferred_fields(
        &mut self,
        kind: SettlementBuildingKind,
        origin: Vec3,
        yaw: f32,
    ) {
        let definition = if kind == SettlementBuildingKind::Hall {
            CivicHallLevel::largest_supported()
                .building_type()
                .definition()
        } else {
            kind.placement_definition()
        };
        self.reserve_rect(
            definition.world_footprint_center(origin, yaw),
            definition.footprint * 0.5,
            yaw,
        );
        if let (Some(pasture), Some(half)) = (
            kind.pasture_position(origin, yaw),
            kind.pasture_half_extents(),
        ) {
            self.reserve_rect(pasture.xz(), half, yaw);
        }
        let entrance = kind.entrance_position(origin, yaw);
        self.reserve_segment(
            entrance.xz(),
            entrance.xz() + local_to_world_xz(Vec2::new(0., -2.), yaw),
            1.0,
        );
    }

    pub fn reserve_road(&mut self, road: &VillageRoad) {
        for pair in road.points.windows(2) {
            self.reserve_segment(
                pair[0],
                pair[1],
                road.reserved_width.max(road.width) * 0.5 + 0.30,
            );
        }
    }

    pub fn reserve_field(&mut self, field: &crate::components::FarmField, origin: Vec3, yaw: f32) {
        let fallback = crate::components::FarmFieldShape::legacy_rectangle();
        let shape = field.shape.as_ref().unwrap_or(&fallback);
        if !shape.is_valid() {
            return;
        }
        for section in shape.sections.windows(2) {
            let [a, b] = [&section[0], &section[1]];
            self.reserve_polygon(
                [
                    Vec2::new(a.left, a.z),
                    Vec2::new(a.right, a.z),
                    Vec2::new(b.right, b.z),
                    Vec2::new(b.left, b.z),
                ]
                .map(|p| origin.xz() + local_to_world_xz(p, yaw))
                .to_vec(),
                crate::components::FARM_FIELD_TERRACE_MARGIN,
            );
        }
    }

    pub fn reserve_yard(&mut self, yard: &HouseholdYard, origin: Vec3, yaw: f32) {
        self.reserve_polygon(
            yard.boundary_points()
                .into_iter()
                .map(|p| origin.xz() + local_to_world_xz(p, yaw))
                .collect(),
            0.30,
        );
    }

    fn nearby(&self, min: Vec2, max: Vec2) -> Vec<&Reservation> {
        let a = cell(min);
        let b = cell(max);
        let mut ids = HashSet::new();
        for x in a.0..=b.0 {
            for z in a.1..=b.1 {
                if let Some(entries) = self.cells.get(&(x, z)) {
                    ids.extend(entries.iter().copied());
                }
            }
        }
        let mut result: Vec<_> = ids.into_iter().map(|i| &self.reservations[i]).collect();
        // HashMap/entity query order must never decide which neighbour wins a
        // clipped corner. Geometry, not transient ECS allocation, orders cuts.
        result.sort_by(|a, b| {
            a.signature.cmp(&b.signature).then_with(|| {
                a.points
                    .iter()
                    .flat_map(|p| p.to_array())
                    .zip(b.points.iter().flat_map(|p| p.to_array()))
                    .find_map(|(a, b)| {
                        let cmp = a.total_cmp(&b);
                        cmp.ne(&std::cmp::Ordering::Equal).then_some(cmp)
                    })
                    .unwrap_or(a.points.len().cmp(&b.points.len()))
            })
        });
        result
    }

    pub fn is_clear(&self, point: Vec2, radius: f32) -> bool {
        // A site fit samples this often: inspect local buckets without allocating.
        let a = cell(point - Vec2::splat(radius));
        let b = cell(point + Vec2::splat(radius));
        for x in a.0..=b.0 {
            for z in a.1..=b.1 {
                if self.cells.get(&(x, z)).is_some_and(|ids| {
                    ids.iter().any(|i| {
                        let r = &self.reservations[*i];
                        geometry::contains(&r.points, point, r.margin + radius)
                    })
                }) {
                    return false;
                }
            }
        }
        true
    }

    pub fn signature_for_yard(&self, yard: &HouseholdYard, origin: Vec3, yaw: f32) -> u64 {
        let (lo, hi) = yard.world_bounds(origin, yaw, 0.50);
        let a = cell(lo);
        let b = cell(hi);
        let mut signature = 0;
        for x in a.0..=b.0 {
            for z in a.1..=b.1 {
                signature = crate::worldgen::splitmix64(
                    signature ^ self.source_cells.get(&(x, z)).copied().unwrap_or_default(),
                );
            }
        }
        signature
    }

    /// Find one connected useful plot rather than squeezing a fixed rectangle
    /// between roads. Clipping deliberately keeps convex parcels: disconnected
    /// leftovers and narrow strips are omitted instead of becoming fake gardens.
    pub fn fit_yard(
        &self,
        appearance: HouseAppearance,
        origin: Vec3,
        yaw: f32,
        seed: u64,
        mut extra_clear: impl FnMut(Vec2, f32) -> bool,
        mut ground: impl FnMut(Vec2) -> Option<f32>,
    ) -> Option<HouseholdYard> {
        let _ = appearance; // future clearance is shared by the known house catalogue
        let definition = SettlementBuildingKind::House.placement_definition();
        let lo = definition.footprint_center - definition.footprint * 0.5;
        let hi = definition.footprint_center + definition.footprint * 0.5;
        let sides = if seed & 1 == 0 {
            [YardSide::Right, YardSide::Rear, YardSide::Left]
        } else {
            [YardSide::Left, YardSide::Rear, YardSide::Right]
        };
        let depth = 3.5 + (seed % 5) as f32 * 0.18;
        let length = 6.1 + ((seed >> 4) % 6) as f32 * 0.21;
        for side in sides {
            for (depth, length) in [(depth, length), (2.8, 5.6), (2.2, 4.8), (1.6, 3.6)] {
                let (min, max) = match side {
                    YardSide::Right => (
                        Vec2::new(hi.x + super::YARD_HOUSE_GAP, -2.2),
                        Vec2::new(hi.x + super::YARD_HOUSE_GAP + depth, -2.2 + length),
                    ),
                    YardSide::Left => (
                        Vec2::new(lo.x - super::YARD_HOUSE_GAP - depth, -2.2),
                        Vec2::new(lo.x - super::YARD_HOUSE_GAP, -2.2 + length),
                    ),
                    YardSide::Rear => (
                        Vec2::new(-length * 0.5, hi.y + super::YARD_HOUSE_GAP),
                        Vec2::new(length * 0.5, hi.y + super::YARD_HOUSE_GAP + depth),
                    ),
                };
                let mut polygon = vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
                let world: Vec<_> = polygon
                    .iter()
                    .map(|p| origin.xz() + local_to_world_xz(*p, yaw))
                    .collect();
                let (wmin, wmax) = geometry::bounds(&world);
                for reservation in self.nearby(wmin - Vec2::ONE, wmax + Vec2::ONE) {
                    let obstacle: Vec<_> = reservation
                        .points
                        .iter()
                        .map(|p| world_to_local_xz(*p - origin.xz(), yaw))
                        .collect();
                    let margin = reservation.margin + 0.46;
                    if !geometry::overlaps(&polygon, &obstacle, margin) {
                        continue;
                    }
                    // Subtracting a convex obstacle can leave several pieces.
                    // Keep the largest piece that still reaches the homeward
                    // working strip; never steal a detached patch across a lane.
                    let mut best: Option<Vec<Vec2>> = None;
                    for (a, b) in geometry::edges(&obstacle) {
                        let e = b - a;
                        let n = Vec2::new(e.y, -e.x).normalize_or_zero();
                        let candidate = geometry::clip(&polygon, -n, -n.dot(a) - margin);
                        if candidate.len() < 3 || geometry::area(&candidate) < 4.5 {
                            continue;
                        }
                        let (cmin, cmax) = geometry::bounds(&candidate);
                        let attached = match side {
                            YardSide::Right => cmin.x <= min.x + 0.75,
                            YardSide::Left => cmax.x >= max.x - 0.75,
                            YardSide::Rear => cmin.y <= min.y + 0.75,
                        };
                        if !attached {
                            continue;
                        }
                        if best
                            .as_ref()
                            .is_none_or(|p| geometry::area(&candidate) > geometry::area(p))
                        {
                            best = Some(candidate);
                        }
                    }
                    polygon = best.unwrap_or_default();
                    if polygon.is_empty() {
                        break;
                    }
                }
                if !geometry::valid_convex(&polygon) {
                    continue;
                }
                let (minimum, maximum) = geometry::bounds(&polygon);
                if (maximum - minimum).min_element() < 1.45 || geometry::area(&polygon) < 5.0 {
                    continue;
                }
                let yard = HouseholdYard {
                    minimum,
                    maximum,
                    side,
                    seed,
                    boundary: polygon,
                    use_kind: match (seed >> 8) % 5 {
                        0 | 1 => YardUse::Vegetables,
                        2 => YardUse::Laundry,
                        3 => YardUse::Flowers,
                        _ => YardUse::Firewood,
                    },
                };
                // Reject a clipped parcel with no embodied working point or
                // no wide open edge. The exact A* tests cover rotated homes.
                let edges = yard.boundary_points();
                let fences = yard.fence_segments();
                let opening = geometry::edges(&edges)
                    .any(|(a, b)| a.distance(b) >= 1.8 && !fences.contains(&(a, b)));
                if !opening || !yard.contains_local_point(yard.center(), -0.45) {
                    continue;
                }
                if yard.fits_site(
                    origin,
                    yaw,
                    |p, r| self.is_clear(p, r) && extra_clear(p, r),
                    &mut ground,
                ) {
                    return Some(yard);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_farm_reserves_new_land_without_expanding_legacy_claims() {
        let at = Vec3::new(61., 2., -63.);
        for step in 0..8 {
            let yaw = step as f32 * std::f32::consts::TAU / 8.;
            let mut legacy = HouseholdYardLand::default();
            legacy.reserve_building(SettlementBuildingKind::Farmstead, at, yaw);
            let mut pending = HouseholdYardLand::default();
            pending.reserve_new_building(SettlementBuildingKind::Farmstead, at, yaw);
            let outer = at.xz() + local_to_world_xz(Vec2::new(12., 29.), yaw);
            assert!(
                legacy.is_clear(outer, 0.4),
                "old farm does not own expanded land"
            );
            assert!(
                !pending.is_clear(outer, 0.4),
                "new farm keeps its proposed crop parcel"
            );
        }
    }

    #[test]
    fn garden_follows_a_diagonal_road_without_claiming_its_access_strip() {
        let origin = Vec3::new(37., 2., -21.);
        let yaw = 0.63;
        let definition = SettlementBuildingKind::House.placement_definition();
        let hi = definition.footprint_center + definition.footprint * 0.5;
        let a = origin.xz() + local_to_world_xz(Vec2::new(hi.x + 2.9, -9.), yaw);
        let b = origin.xz() + local_to_world_xz(Vec2::new(hi.x + 5.2, 10.), yaw);
        let fit = |reverse: bool| {
            let mut land = HouseholdYardLand::default();
            if reverse {
                land.reserve_segment(a, b, 0.55);
            }
            land.reserve_building(SettlementBuildingKind::House, origin, yaw);
            if !reverse {
                land.reserve_segment(a, b, 0.55);
            }
            let yard = land
                .fit_yard(
                    HouseAppearance::default(),
                    origin,
                    yaw,
                    0,
                    |_, _| true,
                    |_| Some(2.),
                )
                .unwrap();
            assert_eq!(yard.side, YardSide::Right);
            assert!(yard.fits_site(origin, yaw, |p, r| land.is_clear(p, r), |_| Some(2.)));
            assert!(
                yard.area()
                    < ((yard.maximum.x - yard.minimum.x) * (yard.maximum.y - yard.minimum.y)) - 0.1,
                "an angled lane must shape the actual parcel, not select another fixed rectangle"
            );
            assert!(
                yard.fence_segments().iter().any(|(a, b)| {
                    let d = *b - *a;
                    d.x.abs() > 0.1 && d.y.abs() > 0.1
                }),
                "a real fence follows the accepted sloping road edge"
            );
            yard
        };
        assert_eq!(
            fit(false),
            fit(true),
            "reservation insertion order must not change land ownership"
        );
    }

    #[test]
    fn neighbouring_parcels_reserve_the_polygon_not_an_invented_box() {
        let yard = HouseholdYard {
            minimum: Vec2::ZERO,
            maximum: Vec2::splat(4.),
            side: YardSide::Right,
            use_kind: YardUse::Flowers,
            seed: 5,
            boundary: vec![
                Vec2::ZERO,
                Vec2::new(4., 0.),
                Vec2::new(2., 4.),
                Vec2::new(0., 4.),
            ],
        };
        let mut land = HouseholdYardLand::default();
        land.reserve_yard(&yard, Vec3::ZERO, 0.);
        assert!(!land.is_clear(Vec2::new(1., 2.), 0.2));
        assert!(
            land.is_clear(Vec2::new(3.9, 3.9), 0.2),
            "unclaimed space outside a clipped edge stays available to neighbours"
        );
    }

    #[test]
    fn explicit_empty_fields_reserve_no_imaginary_legacy_crop_rectangle() {
        let mut field = crate::components::FarmField {
            layout_version: 0,
            settlement: "test".into(),
            farmstead: Vec3::ZERO,
            plot_index: 0,
            quality: 1.,
            shape: Some(crate::components::FarmFieldShape::default()),
        };
        let mut empty = HouseholdYardLand::default();
        empty.reserve_field(&field, Vec3::ZERO, 0.);
        assert!(empty.is_clear(Vec2::ZERO, 0.3));
        field.shape = None;
        let mut legacy = HouseholdYardLand::default();
        legacy.reserve_field(&field, Vec3::ZERO, 0.);
        assert!(!legacy.is_clear(Vec2::ZERO, 0.3));
    }
}
