//! Local built-road context for deterministic household frontage fitting.
//! Ownership exists only to replace cached sources; it never orders a fit.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::components::{distance_squared_to_segment, SettlementBuildingKind, VillageRoad};

const CELL_SIZE: f32 = 16.0;
const SEARCH_RADIUS: f32 = 18.0;
type Cell = (i32, i32);
type SegmentKey = (u64, usize);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frontage {
    pub a: Vec2,
    pub b: Vec2,
    pub width: f32,
}

impl Frontage {
    fn new(mut a: Vec2, mut b: Vec2, width: f32) -> Option<Self> {
        let length_squared = a.distance_squared(b);
        if !a.is_finite()
            || !b.is_finite()
            || !length_squared.is_finite()
            || length_squared == 0.0
            || !width.is_finite()
            || width <= 0.0
        {
            return None;
        }
        // Reversed routes and signed zero describe the same built ground.
        let canonical = |value: f32| if value == 0.0 { 0.0 } else { value };
        a = Vec2::new(canonical(a.x), canonical(a.y));
        b = Vec2::new(canonical(b.x), canonical(b.y));
        if compare_points(a, b).is_gt() {
            std::mem::swap(&mut a, &mut b);
        }
        Some(Self { a, b, width })
    }

    fn compare(&self, other: &Self) -> Ordering {
        compare_points(self.a, other.a)
            .then_with(|| compare_points(self.b, other.b))
            .then_with(|| self.width.total_cmp(&other.width))
    }
}

struct Source {
    segments: Vec<Frontage>,
    cells: HashSet<Cell>,
}

/// Each source retains only its present built segments and occupied cells.
/// Queries visit at most 4 × 4 cells around the shared house entrance, followed
/// by an exact centreline-distance check; no world-wide road scan is needed.
#[derive(Default)]
pub(super) struct RoadFrontages {
    sources: HashMap<u64, Source>,
    cells: HashMap<Cell, Vec<SegmentKey>>,
}

impl RoadFrontages {
    pub(super) fn set(&mut self, owner: u64, road: Option<&VillageRoad>) {
        let mut segments: Vec<_> = road
            .into_iter()
            .flat_map(|road| {
                road.built_points()
                    .windows(2)
                    .filter_map(move |pair| Frontage::new(pair[0], pair[1], road.width))
            })
            .collect();
        segments.sort_by(Frontage::compare);
        segments.dedup();
        if self
            .sources
            .get(&owner)
            .is_some_and(|old| old.segments == segments)
        {
            return;
        }

        if let Some(old) = self.sources.remove(&owner) {
            for cell in old.cells {
                if let Some(entries) = self.cells.get_mut(&cell) {
                    entries.retain(|&(source, _)| source != owner);
                    if entries.is_empty() {
                        self.cells.remove(&cell);
                    }
                }
            }
        }
        if segments.is_empty() {
            return;
        }

        let mut occupied = HashSet::new();
        for (index, segment) in segments.iter().enumerate() {
            visit_segment_cells(segment, |cell| {
                self.cells.entry(cell).or_default().push((owner, index));
                occupied.insert(cell);
            });
        }
        self.sources.insert(
            owner,
            Source {
                segments,
                cells: occupied,
            },
        );
    }

    pub(super) fn nearby(&self, origin: Vec3, yaw: f32) -> Vec<Frontage> {
        if !origin.is_finite() || !yaw.is_finite() {
            return Vec::new();
        }
        let entrance = SettlementBuildingKind::House
            .entrance_position(origin, yaw)
            .xz();
        let minimum = cell(entrance - Vec2::splat(SEARCH_RADIUS));
        let maximum = cell(entrance + Vec2::splat(SEARCH_RADIUS));
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for x in minimum.0..=maximum.0 {
            for z in minimum.1..=maximum.1 {
                if let Some(entries) = self.cells.get(&(x, z)) {
                    for &(owner, index) in entries {
                        if !seen.insert((owner, index)) {
                            continue;
                        }
                        let segment = self.sources[&owner].segments[index];
                        if distance_squared_to_segment(entrance, segment.a, segment.b)
                            <= SEARCH_RADIUS * SEARCH_RADIUS
                        {
                            result.push(segment);
                        }
                    }
                }
            }
        }
        result.sort_by(Frontage::compare);
        // Coincident roads do not become extra frontage when owner IDs change.
        result.dedup();
        result
    }

    /// Geometry-only source signature. Zero explicitly means no nearby built
    /// frontage. Road class, paving, labels and stock do not change this shape;
    /// current width does, while future reservation width remains land's job.
    pub(super) fn signature(&self, origin: Vec3, yaw: f32) -> u64 {
        let nearby = self.nearby(origin, yaw);
        if nearby.is_empty() {
            return 0;
        }
        let mut signature = 0x4652_4F4E_5441_4745 ^ nearby.len() as u64;
        for segment in nearby {
            for value in [
                segment.a.x,
                segment.a.y,
                segment.b.x,
                segment.b.y,
                segment.width,
            ] {
                signature = crate::worldgen::splitmix64(signature ^ u64::from(value.to_bits()));
            }
        }
        signature.max(1)
    }
}

fn compare_points(a: Vec2, b: Vec2) -> Ordering {
    a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y))
}

fn cell(point: Vec2) -> Cell {
    (
        (point.x / CELL_SIZE).floor() as i32,
        (point.y / CELL_SIZE).floor() as i32,
    )
}

/// Traverse crossed cells, not the entire bounding rectangle of a diagonal
/// road. Double-precision crossing parameters avoid accumulated f32 stepping
/// error. Each step advances one axis toward its final cell and then stops.
fn visit_segment_cells(segment: &Frontage, mut visit: impl FnMut(Cell)) {
    let mut current = cell(segment.a);
    let end = cell(segment.b);
    let delta = segment.b.as_dvec2() - segment.a.as_dvec2();
    let step = (delta.x.signum() as i32, delta.y.signum() as i32);
    let first_crossing = |value: f32, cell: i32, direction: f64, step: i32| {
        if direction == 0.0 {
            f64::INFINITY
        } else {
            let boundary = f64::from(cell) + if step > 0 { 1.0 } else { 0.0 };
            (boundary * f64::from(CELL_SIZE) - f64::from(value)) / direction
        }
    };
    let mut next_x = first_crossing(segment.a.x, current.0, delta.x, step.0);
    let mut next_z = first_crossing(segment.a.y, current.1, delta.y, step.1);
    let increment = f64::from(CELL_SIZE) / delta.abs();
    loop {
        visit(current);
        if current == end {
            break;
        }
        if current.1 == end.1 || (current.0 != end.0 && next_x <= next_z) {
            current.0 += step.0;
            next_x += increment.x;
        } else {
            current.1 += step.1;
            next_z += increment.y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{RoadClass, RoadSurface};

    fn road(points: &[(f32, f32)]) -> VillageRoad {
        VillageRoad {
            settlement: "Test".into(),
            builder: "Builder".into(),
            points: points.iter().map(|&(x, z)| Vec2::new(x, z)).collect(),
            built_through: points.len() as u16,
            width: 2.0,
            reserved_width: 4.0,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        }
    }

    fn house_at_entrance(point: Vec2) -> Vec3 {
        let offset = SettlementBuildingKind::House.entrance_position(Vec3::ZERO, 0.0);
        Vec3::new(point.x - offset.x, 0.0, point.y - offset.z)
    }

    #[test]
    fn frontage_uses_only_completed_segments_and_clamps_point_count() {
        let origin = house_at_entrance(Vec2::ZERO);
        let mut cache = RoadFrontages::default();
        let mut path = road(&[(-12.0, 0.0), (0.0, 0.0), (12.0, 0.0)]);
        for built in 0..=3 {
            path.built_through = built;
            cache.set(3, Some(&path));
            assert_eq!(
                cache.nearby(origin, 0.0).len(),
                usize::from(built.saturating_sub(1))
            );
            assert_eq!(cache.signature(origin, 0.0) == 0, built < 2);
        }
        let complete = cache.signature(origin, 0.0);
        path.built_through = u16::MAX;
        cache.set(3, Some(&path));
        assert_eq!(cache.signature(origin, 0.0), complete);
        path.built_through = 1;
        cache.set(3, Some(&path));
        assert!(cache.sources.is_empty() && cache.cells.is_empty());
    }

    #[test]
    fn replacing_or_removing_one_road_clears_only_its_old_cells() {
        let origin = house_at_entrance(Vec2::ZERO);
        let mut cache = RoadFrontages::default();
        let first = road(&[(-8.0, 0.0), (8.0, 0.0)]);
        let second = road(&[(-8.0, 2.0), (8.0, 2.0)]);
        cache.set(1, Some(&first));
        cache.set(2, Some(&second));
        assert_eq!(cache.nearby(origin, 0.0).len(), 2);
        let distant = road(&[(100.0, 100.0), (110.0, 100.0)]);
        cache.set(1, Some(&distant));
        assert_eq!(
            cache.nearby(origin, 0.0),
            vec![Frontage::new(second.points[0], second.points[1], 2.0).unwrap()]
        );
        cache.set(2, None);
        assert_eq!(cache.signature(origin, 0.0), 0);
        assert!(!cache.cells.contains_key(&(-1, 0)));
        assert_eq!(
            cache
                .nearby(house_at_entrance(Vec2::splat(100.0)), 0.0)
                .len(),
            1
        );
        cache.set(1, None);
        cache.set(1, None);
        assert!(cache.sources.is_empty() && cache.cells.is_empty());
    }

    #[test]
    fn queries_use_nearest_segment_points_and_stay_local() {
        let origin = house_at_entrance(Vec2::ZERO);
        let mut cache = RoadFrontages::default();
        let diagonal = road(&[(-1024.0, -1024.0), (1024.0, 1024.0)]);
        cache.set(1, Some(&diagonal));
        assert_eq!(
            cache.nearby(origin, 0.0).len(),
            1,
            "distant endpoints must not hide a nearby middle"
        );
        assert!(
            cache.cells.len() < 300,
            "diagonal indexing must not fill its whole bounding square"
        );
        let signature = cache.signature(origin, 0.0);
        let corner = road(&[(-40.0, 4.0), (4.0, 40.0)]);
        cache.set(2, Some(&corner));
        assert_eq!(
            cache.signature(origin, 0.0),
            signature,
            "an overlapping AABB is not nearby frontage"
        );
        let mut remote = road(&[(500.0, -500.0), (510.0, -500.0)]);
        cache.set(3, Some(&remote));
        remote.width = 6.0;
        cache.set(3, Some(&remote));
        cache.set(3, None);
        assert_eq!(cache.signature(origin, 0.0), signature);
        for point in [-512.0, 0.0, 512.0] {
            assert_eq!(
                cache
                    .nearby(house_at_entrance(Vec2::splat(point)), 0.0)
                    .len(),
                1
            );
        }
    }

    #[test]
    fn geometry_order_ignores_owners_direction_duplicates_and_signed_zero() {
        let origin = house_at_entrance(Vec2::ZERO);
        let a = road(&[(-8.0, 0.0), (8.0, 0.0)]);
        let b = road(&[(-8.0, 4.0), (8.0, 4.0)]);
        let reversed = road(&[(8.0, -0.0), (-8.0, -0.0)]);
        let mut first = RoadFrontages::default();
        first.set(1, Some(&a));
        first.set(2, Some(&b));
        let mut second = RoadFrontages::default();
        second.set(70, Some(&b));
        second.set(60, Some(&reversed));
        second.set(50, Some(&a));
        assert_eq!(first.nearby(origin, 0.0), second.nearby(origin, 0.0));
        assert_eq!(first.signature(origin, 0.0), second.signature(origin, 0.0));
        second.set(60, None);
        assert_eq!(first.signature(origin, 0.0), second.signature(origin, 0.0));
    }

    #[test]
    fn widening_changes_the_signature_but_road_bookkeeping_does_not() {
        let origin = house_at_entrance(Vec2::ZERO);
        let mut cache = RoadFrontages::default();
        let mut path = road(&[(-4.0, 0.0), (4.0, 0.0)]);
        cache.set(9, Some(&path));
        let before = cache.signature(origin, 0.0);
        path.width = 3.0;
        cache.set(9, Some(&path));
        let wider = cache.signature(origin, 0.0);
        assert_ne!(before, wider);
        assert_eq!(cache.nearby(origin, 0.0)[0].width, 3.0);
        path.surface = RoadSurface::Stone;
        path.class = RoadClass::Main;
        path.stone_committed = 40;
        path.reserved_width = 6.0;
        path.builder = "Another builder".into();
        cache.set(9, Some(&path));
        assert_eq!(cache.signature(origin, 0.0), wider);
    }

    #[test]
    fn entrance_orientation_bounds_the_search_and_invalid_geometry_is_ignored() {
        let mut cache = RoadFrontages::default();
        let entrance = SettlementBuildingKind::House.entrance_position(Vec3::ZERO, 0.0);
        let near_front = road(&[(-1.0, entrance.z - 17.5), (1.0, entrance.z - 17.5)]);
        cache.set(1, Some(&near_front));
        assert_eq!(cache.nearby(Vec3::ZERO, 0.0).len(), 1);
        assert!(cache.nearby(Vec3::ZERO, std::f32::consts::PI).is_empty());
        assert!(cache.nearby(Vec3::splat(f32::NAN), 0.0).is_empty());
        assert_eq!(cache.signature(Vec3::ZERO, f32::INFINITY), 0);
        for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut invalid = near_front.clone();
            invalid.width = width;
            cache.set(1, Some(&invalid));
            assert!(cache.sources.is_empty() && cache.cells.is_empty());
        }
        let bad_points = road(&[
            (0.0, 0.0),
            (0.0, 0.0),
            (f32::NAN, 1.0),
            (3.0, 0.0),
            (6.0, 0.0),
        ]);
        cache.set(2, Some(&bad_points));
        let found = cache.nearby(house_at_entrance(Vec2::ZERO), 0.0);
        assert_eq!(
            found,
            vec![Frontage::new(Vec2::new(3.0, 0.0), Vec2::new(6.0, 0.0), 2.0).unwrap()]
        );
    }
}
