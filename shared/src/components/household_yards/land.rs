//! Deterministic nearby land clipping, shared by live authoring and art fixtures.

use super::{geometry, HouseholdYard, YARD_APPROACH_HALF_WIDTH};
use crate::components::{CivicHallLevel, HouseAppearance, SettlementBuildingKind, VillageRoad};
use crate::rotation::local_to_world_xz;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
struct YardSlots {
    parcel: usize,
    approach: usize,
}

pub(super) struct Reservation {
    pub(super) points: Vec<Vec2>,
    pub(super) margin: f32,
    signature: u64,
    pub(super) owner: Option<u64>,
    // Roads and accepted yard approaches remain walkable; they prevent land
    // claims without preventing another household from crossing on foot.
    pub(super) road: bool,
}

/// A yard belongs to its existing house, never to a second independently
/// spawned property entity. These local reservations only decide revocable
/// outdoor space; roads, actual buildings and known upgrades take priority.
#[derive(Default)]
pub struct HouseholdYardLand {
    reservations: Vec<Reservation>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    yard_slots: HashMap<(u64, bool), YardSlots>,
    pub(super) frontages: super::frontage::RoadFrontages,
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / 16.).floor() as i32, (p.y / 16.).floor() as i32)
}

impl HouseholdYardLand {
    fn reserve_polygon(&mut self, points: Vec<Vec2>, margin: f32) {
        self.store_polygon(points, margin, None, false, None);
    }

    fn store_polygon(
        &mut self,
        points: Vec<Vec2>,
        margin: f32,
        owner: Option<u64>,
        road: bool,
        slot: Option<usize>,
    ) -> usize {
        let mut signature = u64::from(margin.to_bits());
        for p in &points {
            for f in p.to_array() {
                signature = crate::worldgen::splitmix64(signature ^ u64::from(f.to_bits()));
            }
        }
        let index = slot.unwrap_or(self.reservations.len());
        if slot.is_some() {
            let old = &self.reservations[index];
            if !old.points.is_empty() {
                let (lo, hi) = geometry::bounds(&old.points);
                let a = cell(lo - Vec2::splat(old.margin));
                let b = cell(hi + Vec2::splat(old.margin));
                for x in a.0..=b.0 {
                    for z in a.1..=b.1 {
                        if let Some(ids) = self.cells.get_mut(&(x, z)) {
                            ids.retain(|id| *id != index);
                            if ids.is_empty() {
                                self.cells.remove(&(x, z));
                            }
                        }
                    }
                }
            }
        }
        if !points.is_empty() {
            let (lo, hi) = geometry::bounds(&points);
            let a = cell(lo - Vec2::splat(margin));
            let b = cell(hi + Vec2::splat(margin));
            for x in a.0..=b.0 {
                for z in a.1..=b.1 {
                    self.cells.entry((x, z)).or_default().push(index);
                }
            }
        }
        let r = Reservation {
            points,
            margin,
            signature,
            owner,
            road,
        };
        if slot.is_some() {
            self.reservations[index] = r;
        } else {
            self.reservations.push(r);
        }
        index
    }

    pub fn set_yard(&mut self, owner: u64, yard: Option<(&HouseholdYard, Vec3, f32)>) {
        self.set_yard_slot(owner, false, yard);
    }
    pub fn set_staged_yard(&mut self, owner: u64, yard: Option<(&HouseholdYard, Vec3, f32)>) {
        self.set_yard_slot(owner, true, yard);
    }
    fn set_yard_slot(
        &mut self,
        owner: u64,
        staged: bool,
        yard: Option<(&HouseholdYard, Vec3, f32)>,
    ) {
        let slot = self.yard_slots.get(&(owner, staged)).copied();
        if yard.is_none() && slot.is_none() {
            return;
        }
        let points = yard.map_or_else(Vec::new, |(yard, origin, yaw)| {
            yard.boundary_points()
                .into_iter()
                .map(|p| origin.xz() + local_to_world_xz(p, yaw))
                .collect()
        });
        let parcel = self.store_polygon(
            points,
            0.30,
            Some(owner),
            false,
            slot.map(|slot| slot.parcel),
        );
        let approach_points = yard
            .and_then(|(yard, origin, yaw)| {
                yard.approach_path().map(|(entry, approach)| {
                    let a = origin.xz() + local_to_world_xz(entry, yaw);
                    let b = origin.xz() + local_to_world_xz(approach, yaw);
                    let along = (b - a).normalize() * YARD_APPROACH_HALF_WIDTH;
                    let side = Vec2::new(-along.y, along.x);
                    vec![
                        a - along - side,
                        b + along - side,
                        b + along + side,
                        a - along + side,
                    ]
                })
            })
            .unwrap_or_default();
        let approach = self.store_polygon(
            approach_points,
            0.0,
            Some(owner),
            true,
            slot.map(|slot| slot.approach),
        );
        self.yard_slots
            .insert((owner, staged), YardSlots { parcel, approach });
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

    pub fn reserve_house(&mut self, appearance: HouseAppearance, origin: Vec3, yaw: f32) {
        let def = appearance.building_type().definition();
        self.reserve_rect(
            def.world_footprint_center(origin, yaw),
            def.footprint * 0.5,
            yaw,
        );
        let entrance = SettlementBuildingKind::House
            .entrance_position(origin, yaw)
            .xz();
        self.reserve_segment(
            entrance,
            entrance + local_to_world_xz(Vec2::new(0., -2.), yaw),
            1.0,
        );
    }

    pub fn reserve_road(&mut self, road: &VillageRoad) {
        let mut owner = 0;
        for p in &road.points {
            for value in p.to_array() {
                owner = crate::worldgen::splitmix64(owner ^ value.to_bits() as u64);
            }
        }
        self.reserve_road_for(owner, road);
    }
    pub fn reserve_road_for(&mut self, owner: u64, road: &VillageRoad) {
        for pair in road.points.windows(2) {
            let delta = pair[1] - pair[0];
            if !delta.is_finite() || delta.length_squared() < 0.0001 {
                continue;
            }
            let half_width = road.reservation_width() * 0.5 + 0.30;
            let along = delta.normalize();
            let side = Vec2::new(-along.y, along.x);
            let a = pair[0] - along * half_width;
            let b = pair[1] + along * half_width;
            self.store_polygon(
                vec![
                    a - side * half_width,
                    b - side * half_width,
                    b + side * half_width,
                    a + side * half_width,
                ],
                0.,
                None,
                true,
                None,
            );
        }
        self.frontages.set(owner, Some(road));
    }
    pub fn replace_frontage(&mut self, owner: u64, road: &VillageRoad) {
        self.frontages.set(owner, Some(road));
    }
    pub fn remove_frontage(&mut self, owner: u64) {
        self.frontages.set(owner, None);
    }
    pub fn frontage_signature(&self, origin: Vec3, yaw: f32) -> u64 {
        self.frontages.signature(origin, yaw)
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
        self.set_yard(
            super::household_yard_seed(origin),
            Some((yard, origin, yaw)),
        );
    }

    pub(super) fn nearby(&self, min: Vec2, max: Vec2) -> Vec<&Reservation> {
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
        self.clear_except(None, point, radius, false)
    }
    pub fn is_clear_for(&self, owner: u64, point: Vec2, radius: f32) -> bool {
        self.clear_except(Some(owner), point, radius, false)
    }
    pub(super) fn clear_except(
        &self,
        owner: Option<u64>,
        point: Vec2,
        radius: f32,
        allow_road: bool,
    ) -> bool {
        let a = cell(point - Vec2::splat(radius));
        let b = cell(point + Vec2::splat(radius));
        for x in a.0..=b.0 {
            for z in a.1..=b.1 {
                if self.cells.get(&(x, z)).is_some_and(|ids| {
                    ids.iter().any(|i| {
                        let r = &self.reservations[*i];
                        !(owner.is_some() && r.owner == owner)
                            && !(allow_road && r.road)
                            && geometry::contains(&r.points, point, r.margin + radius)
                    })
                }) {
                    return false;
                }
            }
        }
        true
    }

    pub fn site_signature(&self, owner: u64, origin: Vec3, radius: f32) -> u64 {
        self.bounds_signature(
            owner,
            origin.xz() - Vec2::splat(radius),
            origin.xz() + Vec2::splat(radius),
        )
    }
    fn bounds_signature(&self, owner: u64, lo: Vec2, hi: Vec2) -> u64 {
        self.nearby(lo, hi)
            .into_iter()
            .filter(|r| r.owner != Some(owner))
            .fold(0u64, |sum, r| sum.wrapping_add(r.signature))
    }
    pub fn signature_for_yard(&self, yard: &HouseholdYard, origin: Vec3, yaw: f32) -> u64 {
        self.signature_for_yard_for(super::household_yard_seed(origin), yard, origin, yaw)
    }
    pub fn signature_for_yard_for(
        &self,
        owner: u64,
        yard: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
    ) -> u64 {
        let (lo, hi) = yard.validation_bounds(origin, yaw, 0.50);
        self.bounds_signature(owner, lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{YardSide, YardUse};
    use crate::rotation::world_to_local_xz;

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
        let definition = HouseAppearance::default().building_type().definition();
        let hi = definition.footprint_center + definition.footprint * 0.5;
        let a = origin.xz() + local_to_world_xz(Vec2::new(hi.x + 2.9, -9.), yaw);
        let b = origin.xz() + local_to_world_xz(Vec2::new(hi.x + 5.2, 10.), yaw);
        let road = crate::components::VillageRoad {
            settlement: "test".into(),
            builder: String::new(),
            points: vec![a, b],
            built_through: 2,
            width: 1.1,
            reserved_width: 1.1,
            surface: crate::components::RoadSurface::Dirt,
            class: crate::components::RoadClass::Lane,
            stone_committed: 0,
        };
        let fit = |reverse: bool| {
            let mut land = HouseholdYardLand::default();
            if reverse {
                land.reserve_road(&road);
            }
            land.reserve_house(HouseAppearance::default(), origin, yaw);
            if !reverse {
                land.reserve_road(&road);
            }
            let yard = land
                .fit_yard(
                    HouseAppearance::default(),
                    origin,
                    yaw,
                    0,
                    |p, _| world_to_local_xz(p - origin.xz(), yaw).x > hi.x,
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
            entry: None,
            approach: None,
            house: None,
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
    fn later_parcel_cannot_claim_a_published_or_staged_households_approach() {
        let candidate = super::super::fit_household_yard(
            HouseAppearance::default(),
            Vec3::ZERO,
            0.,
            0,
            |_, _| true,
            |_| Some(0.),
        )
        .unwrap();
        let point = candidate.center();
        // A previously accepted neighbour sits beyond the prospective parcel;
        // only its external walking route crosses this otherwise free ground.
        let entry = Vec2::new(point.x, candidate.maximum.y + 2.);
        let incumbent = HouseholdYard {
            minimum: entry - Vec2::X,
            maximum: entry + Vec2::new(1., 2.),
            side: YardSide::Right,
            use_kind: YardUse::Flowers,
            seed: 13,
            boundary: Vec::new(),
            entry: Some(entry),
            approach: Some(point),
            house: None,
        };
        assert!(incumbent.approach_path().is_some());
        assert!(!incumbent.contains_local_point(point, 0.3));
        for staged in [false, true] {
            let mut land = HouseholdYardLand::default();
            let accepted = |land: &HouseholdYardLand| {
                candidate.fits_site(
                    Vec3::ZERO,
                    0.,
                    |p, r| land.is_clear_for(2, p, r),
                    |_| Some(0.),
                )
            };
            assert!(accepted(&land));
            land.set_yard_slot(1, staged, Some((&incumbent, Vec3::ZERO, 0.)));
            assert!(
                !accepted(&land),
                "later garden must not fence off an existing approach"
            );
            assert!(
                land.is_clear_for(1, point, 0.4),
                "owner can refit its own land"
            );
            assert!(
                land.clear_except(Some(2), point, 0.4, true),
                "approaches remain walkable"
            );
            let slot_count = land.reservations.len();
            land.set_yard_slot(1, staged, Some((&incumbent, Vec3::X * 40., 0.)));
            assert!(accepted(&land), "replacement releases the old corridor");
            assert!(!land.is_clear_for(2, point + Vec2::X * 40., 0.4));
            land.set_yard_slot(1, staged, None);
            assert!(land.is_clear_for(2, point + Vec2::X * 40., 0.4));
            assert_eq!(
                land.reservations.len(),
                slot_count,
                "replacements reuse both slots"
            );
        }
    }

    #[test]
    fn validation_bounds_include_rotated_approach_caps_and_external_land_changes() {
        let yard = HouseholdYard {
            minimum: Vec2::new(5., 2.),
            maximum: Vec2::new(9., 6.),
            side: YardSide::Right,
            use_kind: YardUse::Flowers,
            seed: 1,
            boundary: Vec::new(),
            entry: Some(Vec2::new(7., 2.)),
            approach: Some(Vec2::new(3., -6.)),
            house: None,
        };
        let origin = Vec3::new(10., 0., 8.);
        for step in 0..16 {
            let yaw = step as f32 * std::f32::consts::TAU / 16.;
            let mut land = HouseholdYardLand::default();
            land.set_yard(1, Some((&yard, origin, yaw)));
            let slots = land.yard_slots[&(1, false)];
            let (lo, hi) = yard.validation_bounds(origin, yaw, 0.);
            for point in &land.reservations[slots.approach].points {
                assert!(point.cmpge(lo - Vec2::splat(0.0001)).all());
                assert!(point.cmple(hi + Vec2::splat(0.0001)).all());
            }
        }
        let mut land = HouseholdYardLand::default();
        let before = land.signature_for_yard_for(1, &yard, origin, 0.);
        let target = origin.xz() + yard.approach.unwrap();
        land.reserve_rect(target, Vec2::splat(0.2), 0.);
        assert_ne!(before, land.signature_for_yard_for(1, &yard, origin, 0.));
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
