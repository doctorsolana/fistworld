//! Street-led parcel candidates. All clipping and ranking happens at authoring
//! time; clients receive the accepted polygon and entrance, never infer land.

use super::{geometry, project_segment, HouseholdYard, HouseholdYardLand, YardSide, YardUse};
use crate::components::HouseAppearance;
use crate::rotation::{local_to_world_xz, world_to_local_xz};
use bevy::prelude::*;

fn homeward(side: YardSide, normal: Vec2) -> bool {
    match side {
        YardSide::Left => normal.x > 0.80,
        YardSide::Right => normal.x < -0.80,
        YardSide::Rear => normal.y < -0.80,
    }
}

/// A continuous local street run, independent of how many polyline points the
/// survey used. Only connected, approximately collinear built segments extend
/// its support; a short driveway cannot borrow frontage from a remote road.
fn street_support(road: [Vec2; 2], roads: &[[Vec2; 2]], axis: Vec2) -> (f32, f32) {
    let direction = (road[1] - road[0]).normalize();
    let mut intervals: Vec<_> = roads
        .iter()
        .filter_map(|r| {
            let d = (r[1] - r[0]).normalize();
            (d.dot(direction).abs() >= 0.96
                && direction.perp_dot(r[0] - road[0]).abs() <= 1.2
                && direction.perp_dot(r[1] - road[0]).abs() <= 1.2)
                .then(|| {
                    let a = r[0].dot(axis);
                    let b = r[1].dot(axis);
                    (a.min(b), a.max(b))
                })
        })
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let anchor = ((road[0] + road[1]) * 0.5).dot(axis);
    let mut run: Option<(f32, f32)> = None;
    for (lo, hi) in intervals {
        match run {
            Some((a, b)) if lo <= b + 0.05 => run = Some((a, b.max(hi))),
            Some((a, b)) if anchor >= a && anchor <= b => return (a, b),
            _ => run = Some((lo, hi)),
        }
    }
    run.unwrap_or((anchor, anchor))
}

/// At most two generous envelopes per side: one facing a side lane, one
/// reaching a street past either end of the house. Real street distance and
/// connected frontage determine their extents; the exact reserved polygons
/// below still do every final corner cut. No decorative boundary noise.
fn street_envelopes(lo: Vec2, hi: Vec2, side: YardSide, roads: &[[Vec2; 2]]) -> Vec<(Vec2, Vec2)> {
    let (normal, tangent) = match side {
        YardSide::Right => (Vec2::X, Vec2::Y),
        YardSide::Left => (-Vec2::X, Vec2::Y),
        YardSide::Rear => (Vec2::Y, Vec2::X),
    };
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    let attach = corners
        .iter()
        .map(|p| p.dot(normal))
        .fold(f32::NEG_INFINITY, f32::max)
        + super::YARD_HOUSE_GAP;
    let vlo = corners
        .iter()
        .map(|p| p.dot(tangent))
        .fold(f32::INFINITY, f32::min);
    let vhi = corners
        .iter()
        .map(|p| p.dot(tangent))
        .fold(f32::NEG_INFINITY, f32::max);
    let base = normal * attach + tangent * ((vlo + vhi) * 0.5);
    let mut result = Vec::with_capacity(2);
    for side_lane in [true, false] {
        let mut eligible: Vec<_> = roads
            .iter()
            .filter(|r| {
                let point = project_segment(base, r[0], r[1]);
                let direction = (r[1] - r[0]).normalize();
                if side_lane {
                    point.dot(normal) > attach + 1.5 && direction.dot(tangent).abs() > 0.65
                } else {
                    (point.dot(tangent) < vlo - 0.5 || point.dot(tangent) > vhi + 0.5)
                        && direction.dot(normal).abs() > 0.55
                }
            })
            .collect();
        eligible.sort_by(|a, b| {
            let distance =
                |r: &&[Vec2; 2]| project_segment(base, r[0], r[1]).distance_squared(base);
            let key = |r: &&[Vec2; 2]| {
                let order = |a: Vec2, b: Vec2| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y));
                if order(r[0], r[1]).is_le() {
                    [r[0], r[1]]
                } else {
                    [r[1], r[0]]
                }
            };
            distance(a).total_cmp(&distance(b)).then_with(|| {
                for (a, b) in key(a).into_iter().zip(key(b)) {
                    let order = a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y));
                    if !order.is_eq() {
                        return order;
                    }
                }
                std::cmp::Ordering::Equal
            })
        });
        // A nearby short spur can point in the right direction without
        // extending alongside this house at all. Reject that envelope and try
        // the next real street, not the generic small rectangle immediately.
        for &road in eligible {
            let delta = road[1] - road[0];
            let (far, mut begin, mut end) = if side_lane {
                let (a, b) = street_support(road, roads, tangent);
                let begin = (vlo - 2.4).max(a);
                let end = (vhi + 3.8).min(b);
                let road_u = |v: f32| {
                    let t = (v - road[0].dot(tangent)) / delta.dot(tangent);
                    (road[0] + delta * t).dot(normal)
                };
                (road_u(begin).max(road_u(end)).min(attach + 10.), begin, end)
            } else {
                let (_, street_end) = street_support(road, roads, normal);
                let point = project_segment(base, road[0], road[1]);
                let street_v = point.dot(tangent);
                let gap = (vlo - street_v).max(street_v - vhi).max(0.);
                let depth = (3.3 + gap * 0.6).clamp(3.3, 8.8);
                let far = (attach + depth).min(street_end);
                let road_v = |u: f32| {
                    let t = (u - road[0].dot(normal)) / delta.dot(normal);
                    (road[0] + delta * t).dot(tangent)
                };
                if street_v < (vlo + vhi) * 0.5 {
                    (far, road_v(attach).min(road_v(far)), vhi + depth * 0.35)
                } else {
                    (far, vlo - depth * 0.35, road_v(attach).max(road_v(far)))
                }
            };
            // Keep the full frontage-facing end when limiting the opposite extent.
            if end - begin > 14.4 {
                let street_v = project_segment(base, road[0], road[1]).dot(tangent);
                if street_v < (vlo + vhi) * 0.5 {
                    end = begin + 14.4;
                } else {
                    begin = end - 14.4;
                }
            }
            if far - attach < 1.6 || end - begin < 2.4 {
                continue;
            }
            let points = [
                normal * attach + tangent * begin,
                normal * far + tangent * begin,
                normal * far + tangent * end,
                normal * attach + tangent * end,
            ];
            result.push(geometry::bounds(&points));
            break;
        }
    }
    result
}

impl HouseholdYardLand {
    /// Verify the published entrance against today's built street and obstacles.
    /// Access lies partly outside the parcel, so parcel validation alone cannot
    /// catch a later building or neighbour cutting off the approach.
    #[allow(clippy::too_many_arguments)]
    pub fn yard_access_is_clear_for(
        &self,
        owner: u64,
        yard: &HouseholdYard,
        origin: Vec3,
        yaw: f32,
        mut extra_clear: impl FnMut(Vec2, f32) -> bool,
        mut ground: impl FnMut(Vec2) -> Option<f32>,
    ) -> bool {
        let Some((entry, approach)) = yard.approach_path() else {
            return false;
        };
        let world = |p| origin.xz() + local_to_world_xz(p, yaw);
        let entry_world = world(entry);
        let mut fences = crate::spatial::SpatialObstacleGrid::default();
        for obstacle in yard.ground_obstacles(origin, yaw) {
            fences.insert(obstacle);
        }
        if fences.segment_blocked(world(yard.center()), entry_world) {
            return false;
        }
        let boundary = yard.boundary_points();
        let Some((a, b)) = geometry::edges(&boundary)
            .find(|(a, b)| entry.distance(project_segment(entry, *a, *b)) < 0.02)
        else {
            return false;
        };
        let edge = b - a;
        let outward = local_to_world_xz(Vec2::new(edge.y, -edge.x).normalize_or_zero(), yaw);
        let target = world(approach);
        // Do not silently choose a new route after publication: the reserved
        // corridor and the route checked here must describe the same ground.
        let on_built_street = self.frontages.nearby(origin, yaw).iter().any(|road| {
            target.distance_squared(project_segment(target, road.a, road.b))
                <= (road.width * 0.5 + 0.02).powi(2)
        });
        let direction = target - entry_world;
        let distance = direction.length();
        if !on_built_street
            || distance > 10.01
            || outward.dot(direction.normalize_or_zero()) < 0.15
            || fences.segment_blocked(entry_world, target)
        {
            return false;
        }
        let steps = (distance / 0.4).ceil().max(1.) as usize;
        (0..=steps).all(|i| {
            let point = entry_world.lerp(target, i as f32 / steps as f32);
            self.clear_except(Some(owner), point, 0.55, true)
                && extra_clear(point, 0.55)
                && ground(point).is_some_and(|h| h.is_finite())
        })
    }

    pub fn fit_yard(
        &self,
        appearance: HouseAppearance,
        origin: Vec3,
        yaw: f32,
        seed: u64,
        extra_clear: impl FnMut(Vec2, f32) -> bool,
        ground: impl FnMut(Vec2) -> Option<f32>,
    ) -> Option<HouseholdYard> {
        self.fit_yard_for(
            super::household_yard_seed(origin),
            appearance,
            origin,
            yaw,
            seed,
            extra_clear,
            ground,
        )
    }

    /// Evaluate a fixed set of usable side/rear plots against real built streets.
    /// Rank area, useful road-facing boundary and short clear access together.
    /// A small first-fit rectangle must not win over a coherent street-side plot.
    #[allow(clippy::too_many_arguments)]
    pub fn fit_yard_for(
        &self,
        owner: u64,
        appearance: HouseAppearance,
        origin: Vec3,
        yaw: f32,
        seed: u64,
        mut extra_clear: impl FnMut(Vec2, f32) -> bool,
        mut ground: impl FnMut(Vec2) -> Option<f32>,
    ) -> Option<HouseholdYard> {
        let roads = self.frontages.nearby(origin, yaw);
        if roads.is_empty() {
            return None;
        }
        let house = appearance.building_type().definition();
        let lo = house.footprint_center - house.footprint * 0.5;
        let hi = house.footprint_center + house.footprint * 0.5;
        let world = |p| origin.xz() + local_to_world_xz(p, yaw);
        let local = |p| world_to_local_xz(p - origin.xz(), yaw);
        let local_roads: Vec<_> = roads
            .iter()
            .map(|road| [local(road.a), local(road.b)])
            .collect();
        let sides = if seed & 1 == 0 {
            [YardSide::Right, YardSide::Rear, YardSide::Left]
        } else {
            [YardSide::Left, YardSide::Rear, YardSide::Right]
        };
        let mut candidates = Vec::with_capacity(15);
        for (preference, side) in sides.into_iter().enumerate() {
            let mut envelopes = street_envelopes(lo, hi, side, &local_roads);
            let street_sized = envelopes.len();
            // Constrained terrain/props may reject the street-sized parcel.
            // Three compact fallbacks retain a usable small yard or omit it.
            for (depth, length, front) in
                [(4.2, 8.0, lo.y - 0.6), (2.8, 6.0, -2.2), (1.8, 4.5, -1.8)]
            {
                envelopes.push(match side {
                    YardSide::Right => (
                        Vec2::new(hi.x + super::YARD_HOUSE_GAP, front),
                        Vec2::new(hi.x + super::YARD_HOUSE_GAP + depth, front + length),
                    ),
                    YardSide::Left => (
                        Vec2::new(lo.x - super::YARD_HOUSE_GAP - depth, front),
                        Vec2::new(lo.x - super::YARD_HOUSE_GAP, front + length),
                    ),
                    YardSide::Rear => (
                        Vec2::new(-length * 0.5, hi.y + super::YARD_HOUSE_GAP),
                        Vec2::new(length * 0.5, hi.y + super::YARD_HOUSE_GAP + depth),
                    ),
                });
            }
            for (envelope_index, (min, max)) in envelopes.into_iter().enumerate() {
                let mut polygon = vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
                let bounds: Vec<_> = polygon.iter().map(|p| world(*p)).collect();
                let (wmin, wmax) = geometry::bounds(&bounds);
                for reservation in self.nearby(wmin - Vec2::ONE, wmax + Vec2::ONE) {
                    if reservation.owner == Some(owner) {
                        continue;
                    }
                    let obstacle: Vec<_> = reservation.points.iter().map(|p| local(*p)).collect();
                    let margin = reservation.margin + 0.46;
                    if !geometry::overlaps(&polygon, &obstacle, margin) {
                        continue;
                    }
                    let mut best: Option<Vec<Vec2>> = None;
                    for (a, b) in geometry::edges(&obstacle) {
                        let edge = b - a;
                        let n = Vec2::new(edge.y, -edge.x).normalize_or_zero();
                        let piece = geometry::clip(&polygon, -n, -n.dot(a) - margin);
                        if piece.len() < 3 || geometry::area(&piece) < 5. {
                            continue;
                        }
                        let (cmin, cmax) = geometry::bounds(&piece);
                        let attached = match side {
                            YardSide::Right => cmin.x <= min.x + 0.65,
                            YardSide::Left => cmax.x >= max.x - 0.65,
                            YardSide::Rear => cmin.y <= min.y + 0.65,
                        };
                        if attached
                            && best
                                .as_ref()
                                .is_none_or(|p| geometry::area(&piece) > geometry::area(p))
                        {
                            best = Some(piece);
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
                let area = geometry::area(&polygon);
                if (maximum - minimum).min_element() < 1.6 || area < 6.0 {
                    continue;
                }
                let mut yard = HouseholdYard {
                    minimum,
                    maximum,
                    side,
                    seed,
                    boundary: polygon,
                    entry: None,
                    approach: None,
                    house: Some(appearance),
                    use_kind: match (seed >> 8) % 5 {
                        0 | 1 => YardUse::Vegetables,
                        2 => YardUse::Laundry,
                        3 => YardUse::Flowers,
                        _ => YardUse::Firewood,
                    },
                };
                if !yard.contains_local_point(yard.center(), -0.65) {
                    continue;
                }
                let mut access: Option<(Vec2, Vec2, f32)> = None;
                // The accepted entrance must connect to a built road without
                // crossing somebody else's plot, a building, water or a prop.
                for (a, b) in geometry::edges(&yard.boundary) {
                    let delta = b - a;
                    let length = delta.length();
                    if length < 1.8 {
                        continue;
                    }
                    let outward = Vec2::new(delta.y, -delta.x) / length;
                    if homeward(side, outward) {
                        continue;
                    }
                    for road in &roads {
                        let road_point = project_segment(world((a + b) * 0.5), road.a, road.b);
                        let target = local(road_point);
                        let t = ((target - a).dot(delta) / delta.length_squared())
                            .clamp((1.35 / length).min(0.5), 1. - (1.35 / length).min(0.5));
                        let entry = a + delta * t;
                        let approach = target - entry;
                        let distance = approach.length();
                        if distance > 10. || outward.dot(approach.normalize_or_zero()) < 0.15 {
                            continue;
                        }
                        let road_dir = local(road.b) - local(road.a);
                        let parallel = (delta.normalize().dot(road_dir.normalize_or_zero())).abs();
                        let score = length.min(10.) * (0.4 + parallel) * 1.3 - distance * 1.8;
                        if access.is_some_and(|(_, _, best)| score <= best) {
                            continue;
                        }
                        let n = (distance / 0.4).ceil().max(1.) as usize;
                        let clear = (0..=n).all(|i| {
                            let p = world(entry.lerp(target, i as f32 / n as f32));
                            self.clear_except(Some(owner), p, 0.55, true)
                                && extra_clear(p, 0.55)
                                && ground(p).is_some_and(|h| h.is_finite())
                        });
                        if !clear {
                            continue;
                        }
                        if access.is_none_or(|(_, _, s)| score > s) {
                            access = Some((entry, target, score));
                        }
                    }
                }
                let Some((entry, approach, frontage)) = access else {
                    continue;
                };
                yard.entry = Some(entry);
                yard.approach = Some(approach);
                let score = area.sqrt() * 4.0 + frontage - preference as f32 * 0.18;
                candidates.push((envelope_index < street_sized, score, yard));
            }
        }
        // A larger generic fallback must not undo a deliberately compact street
        // frontage. Use it only when every street-sized candidate is invalid.
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
        for (_, _, yard) in candidates {
            if yard.fits_site(
                origin,
                yaw,
                |p, r| self.is_clear_for(owner, p, r) && extra_clear(p, r),
                &mut ground,
            ) && self.yard_access_is_clear_for(
                owner,
                &yard,
                origin,
                yaw,
                &mut extra_clear,
                &mut ground,
            ) {
                return Some(yard);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{HouseLevel, HouseLine, RoadClass, RoadSurface, VillageRoad};
    use crate::spatial::{ObstacleEntry, SpatialObstacleGrid};

    fn street(origin: Vec3, yaw: f32) -> VillageRoad {
        VillageRoad {
            settlement: "test".into(),
            builder: String::new(),
            points: [Vec2::new(-24., -8.), Vec2::new(24., -8.)]
                .map(|p| origin.xz() + local_to_world_xz(p, yaw))
                .to_vec(),
            built_through: 2,
            width: 2.,
            reserved_width: 4.,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        }
    }

    #[test]
    fn actual_street_sites_produce_compact_long_narrow_and_deep_envelopes() {
        let lo = Vec2::new(-2., -3.);
        let hi = Vec2::new(2., 3.);
        let cases = [
            [Vec2::new(9., -3.), Vec2::new(9., 3.)],
            [Vec2::new(6.5, -24.), Vec2::new(6.5, 24.)],
            [Vec2::new(-24., -11.), Vec2::new(24., -11.)],
        ];
        let sizes: Vec<_> = cases
            .iter()
            .map(|road| {
                let envelopes = street_envelopes(lo, hi, YardSide::Right, &[*road]);
                assert_eq!(envelopes.len(), 1);
                envelopes[0].1 - envelopes[0].0
            })
            .collect();
        assert!(sizes[0].y < 6.1, "short frontage creates a compact plot");
        assert!(
            sizes[1].y > sizes[0].y * 1.8,
            "long lane grows length, not arbitrary depth"
        );
        assert!(
            sizes[1].x < sizes[0].x - 2.,
            "near side street bounds actual depth"
        );
        assert!(
            sizes[2].x > sizes[1].x * 1.8,
            "open land across the front street permits a deeper yard"
        );
        assert!(sizes.iter().all(|size| size.max_element() <= 14.5));
    }

    #[test]
    fn street_polyline_subdivision_does_not_change_household_extent() {
        let lo = Vec2::new(-2., -3.);
        let hi = Vec2::new(2., 3.);
        let whole = [[Vec2::new(-24., -8.), Vec2::new(24., -8.)]];
        let split = [
            [Vec2::new(24., -8.), Vec2::new(8., -8.)],
            [Vec2::new(-8., -8.), Vec2::new(-24., -8.)],
            [Vec2::new(0., -8.), Vec2::new(8., -8.)],
            [Vec2::new(0., -8.), Vec2::new(-8., -8.)],
        ];
        for side in [YardSide::Right, YardSide::Left, YardSide::Rear] {
            let a = street_envelopes(lo, hi, side, &whole);
            let b = street_envelopes(lo, hi, side, &split);
            assert_eq!(a.len(), b.len());
            for ((lo_a, hi_a), (lo_b, hi_b)) in a.into_iter().zip(b) {
                assert!(lo_a.distance(lo_b) < 0.0001 && hi_a.distance(hi_b) < 0.0001);
            }
        }
    }

    #[test]
    fn an_unusable_nearest_spur_does_not_hide_a_useful_street_in_the_same_direction() {
        let lo = Vec2::new(-2., -3.);
        let hi = Vec2::new(2., 3.);
        for (spur, street) in [
            // Front spur points towards the other side of the house and ends
            // before any of this side's potential yard frontage.
            (
                [Vec2::new(-2., -6.), Vec2::new(0., -6.)],
                [Vec2::new(-20., -8.), Vec2::new(20., -8.)],
            ),
            // Side spur has too little continuous frontage for a usable gate.
            (
                [Vec2::new(5.5, 5.), Vec2::new(5.5, 6.)],
                [Vec2::new(9., -10.), Vec2::new(9., 10.)],
            ),
        ] {
            assert!(street_envelopes(lo, hi, YardSide::Right, &[spur]).is_empty());
            let expected = street_envelopes(lo, hi, YardSide::Right, &[street]);
            assert_eq!(expected.len(), 1);
            let with_spur = street_envelopes(lo, hi, YardSide::Right, &[spur, street]);
            assert_eq!(expected, with_spur);
            assert_eq!(
                expected,
                street_envelopes(
                    lo,
                    hi,
                    YardSide::Right,
                    &[[street[1], street[0]], [spur[1], spur[0]],]
                ),
                "point direction and insertion order cannot restore the masking bug"
            );
        }
    }

    #[test]
    fn a_second_angled_street_shapes_a_real_corner_parcel_at_the_same_seed() {
        let fit = |corner: bool, reverse: bool| {
            let mut land = HouseholdYardLand::default();
            let appearance = HouseAppearance::default();
            land.reserve_house(appearance, Vec3::ZERO, 0.);
            let front = street(Vec3::ZERO, 0.);
            let side = VillageRoad {
                points: vec![Vec2::new(10., -10.), Vec2::new(7., 12.)],
                width: 1.,
                reserved_width: 1.,
                ..front.clone()
            };
            if corner && reverse {
                land.reserve_road_for(2, &side);
            }
            land.reserve_road_for(1, &front);
            if corner && !reverse {
                land.reserve_road_for(2, &side);
            }
            let yard = land
                .fit_yard_for(
                    7,
                    appearance,
                    Vec3::ZERO,
                    0.,
                    42,
                    |p, _| p.x > 0.,
                    |_| Some(0.),
                )
                .unwrap();
            assert!(yard.fits_site(
                Vec3::ZERO,
                0.,
                |p, r| land.is_clear_for(7, p, r),
                |_| Some(0.)
            ));
            assert!(land.yard_access_is_clear_for(
                7,
                &yard,
                Vec3::ZERO,
                0.,
                |_, _| true,
                |_| Some(0.)
            ));
            yard
        };
        let open = fit(false, false);
        let corner = fit(true, false);
        assert!(corner.area() < open.area() - 1.);
        assert!(
            geometry::edges(&corner.boundary).any(|(a, b)| {
                let d = b - a;
                d.x.abs() > 0.1 && d.y.abs() > 0.1
            }),
            "the accepted fence boundary must actually follow the angled street"
        );
        assert_eq!(
            corner,
            fit(true, true),
            "reservation insertion order cannot pick another shape"
        );
    }

    #[test]
    fn built_street_enables_a_useful_plot_and_planned_street_does_not() {
        let mut land = HouseholdYardLand::default();
        let appearance = HouseAppearance::default();
        let mut road = street(Vec3::ZERO, 0.);
        road.built_through = 1;
        land.reserve_house(appearance, Vec3::ZERO, 0.);
        land.reserve_road_for(9, &road);
        let fit = |land: &HouseholdYardLand| {
            land.fit_yard_for(7, appearance, Vec3::ZERO, 0., 42, |_, _| true, |_| Some(0.))
        };
        assert!(fit(&land).is_none());
        road.built_through = 2;
        land.replace_frontage(9, &road);
        let yard = fit(&land).expect("built accessible street must allow a yard");
        assert!(yard.area() > 35., "prefer a coherent usable plot: {yard:?}");
        assert_eq!(yard.house, Some(appearance));
        assert!(yard.entry.is_some());
        land.remove_frontage(9);
        assert!(fit(&land).is_none());
    }

    #[test]
    fn accepted_gates_and_house_doors_are_walkable_at_every_rotation_and_level() {
        for line in [HouseLine::Cabin, HouseLine::LongCabin] {
            for level in [HouseLevel::Ground, HouseLevel::UpperStorey] {
                for step in 0..16 {
                    let appearance = HouseAppearance { line, level };
                    let yaw = step as f32 * std::f32::consts::TAU / 16.;
                    let origin = Vec3::new(37.3, 2., -21.7);
                    let mut land = HouseholdYardLand::default();
                    land.reserve_house(appearance, origin, yaw);
                    let road = street(origin, yaw);
                    land.reserve_road(&road);
                    let mut yard = land
                        .fit_yard_for(7, appearance, origin, yaw, step, |_, _| true, |_| Some(2.))
                        .unwrap();
                    yard.use_kind = YardUse::Firewood;
                    let world = |p| origin.xz() + local_to_world_xz(p, yaw);
                    let entry = yard.entry.unwrap();
                    let street =
                        world(yard.approach.expect("accepted route has a street endpoint"));
                    let definition = appearance.building_type().definition();
                    let mut grid = SpatialObstacleGrid::default();
                    grid.insert(ObstacleEntry {
                        center: definition.world_footprint_center(origin, yaw),
                        half_extents: definition.footprint * 0.5
                            + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
                        rotation: yaw,
                        obstacle_type: appearance.building_type() as u32,
                    });
                    for obstacle in yard.ground_obstacles(origin, yaw) {
                        grid.insert(obstacle);
                    }
                    for segment in [world(yard.center()), world(entry), street].windows(2) {
                        assert!(
                            !grid.segment_blocked(segment[0], segment[1]),
                            "{appearance:?} yaw {yaw} gate path blocked: {yard:?}"
                        );
                    }
                    let door = crate::components::SettlementBuildingKind::House
                        .entrance_position(origin, yaw)
                        .xz();
                    assert!(!grid
                        .segment_blocked(door, door + local_to_world_xz(Vec2::new(0., -4.), yaw)));
                    assert!(!yard.planting_clear(entry, 0.1));
                    assert!(!yard.planting_clear(entry.lerp(yard.center(), 0.5), 0.1));
                }
            }
        }
    }

    #[test]
    fn upgrading_refits_actual_house_envelope_instead_of_leaving_a_permanent_moat() {
        let fit = |appearance: HouseAppearance| {
            let mut land = HouseholdYardLand::default();
            land.reserve_house(appearance, Vec3::ZERO, 0.);
            land.reserve_road(&street(Vec3::ZERO, 0.));
            land.fit_yard_for(
                1,
                appearance,
                Vec3::ZERO,
                0.,
                0,
                |p, _| p.x > 0.,
                |_| Some(0.),
            )
            .unwrap()
        };
        let small = HouseAppearance {
            line: HouseLine::Cabin,
            level: HouseLevel::Ground,
        };
        let upper = HouseAppearance {
            line: HouseLine::Cabin,
            level: HouseLevel::UpperStorey,
        };
        let lower_yard = fit(small);
        let upper_yard = fit(upper);
        assert_eq!(lower_yard.side, YardSide::Right);
        assert!(upper_yard.minimum.x > lower_yard.minimum.x + 0.20);
        let mut stale = lower_yard.clone();
        stale.house = Some(upper);
        assert!(!stale.fits_site(Vec3::ZERO, 0., |_, _| true, |_| Some(0.)));
        let mut invalid = upper_yard.clone();
        invalid.entry = Some(invalid.center());
        assert!(!invalid.fits_site(Vec3::ZERO, 0., |_, _| true, |_| Some(0.)));
        invalid.entry = Some(Vec2::splat(f32::NAN));
        assert!(!invalid.fits_site(Vec3::ZERO, 0., |_, _| true, |_| Some(0.)));
        invalid = upper_yard.clone();
        invalid.approach = Some(Vec2::splat(f32::INFINITY));
        assert!(!invalid.fits_site(Vec3::ZERO, 0., |_, _| true, |_| Some(0.)));
        let bytes = bincode::serialize(&upper_yard).unwrap();
        assert_eq!(
            upper_yard,
            bincode::deserialize::<HouseholdYard>(&bytes).unwrap()
        );
    }

    #[test]
    fn stored_approach_is_not_silently_redirected_when_a_built_street_moves() {
        let mut land = HouseholdYardLand::default();
        let appearance = HouseAppearance::default();
        land.reserve_house(appearance, Vec3::ZERO, 0.);
        let mut road = street(Vec3::ZERO, 0.);
        land.reserve_road_for(9, &road);
        let yard = land
            .fit_yard_for(7, appearance, Vec3::ZERO, 0., 42, |_, _| true, |_| Some(0.))
            .unwrap();
        let valid = |land: &HouseholdYardLand| {
            land.yard_access_is_clear_for(7, &yard, Vec3::ZERO, 0., |_, _| true, |_| Some(0.))
        };
        assert!(valid(&land));
        // The moved street is still nearby and reachable, but the accepted
        // corridor no longer reaches its actual built surface.
        for point in &mut road.points {
            point.y -= 2.;
        }
        land.replace_frontage(9, &road);
        assert!(!valid(&land));
        for point in &mut road.points {
            point.y += 2.;
        }
        land.replace_frontage(9, &road);
        assert!(valid(&land));
        road.built_through = 1;
        land.replace_frontage(9, &road);
        assert!(!valid(&land));
    }

    #[test]
    fn published_and_staged_land_ignore_self_but_both_protect_their_neighbours() {
        let mut land = HouseholdYardLand::default();
        let appearance = HouseAppearance::default();
        land.reserve_house(appearance, Vec3::ZERO, 0.);
        land.reserve_road(&street(Vec3::ZERO, 0.));
        let yard = land
            .fit_yard_for(7, appearance, Vec3::ZERO, 0., 0, |_, _| true, |_| Some(0.))
            .unwrap();
        let initial = land.site_signature(7, Vec3::ZERO, 18.);
        land.set_yard(7, Some((&yard, Vec3::ZERO, 0.)));
        land.set_staged_yard(7, Some((&yard, Vec3::X * 30., 0.)));
        assert_eq!(initial, land.site_signature(7, Vec3::ZERO, 18.));
        assert!(land.is_clear_for(7, yard.center(), 0.4));
        assert!(!land.is_clear_for(8, yard.center(), 0.4));
        assert!(!land.is_clear_for(8, yard.center() + Vec2::X * 30., 0.4));
        land.set_staged_yard(7, Some((&yard, Vec3::X * 50., 0.)));
        assert!(land.is_clear_for(8, yard.center() + Vec2::X * 30., 0.4));
        land.set_yard(7, None);
        assert!(land.is_clear_for(8, yard.center(), 0.4));
        assert!(!land.is_clear_for(8, yard.center() + Vec2::X * 50., 0.4));
        land.set_staged_yard(7, None);
        assert!(land.is_clear_for(8, yard.center() + Vec2::X * 50., 0.4));
        assert_eq!(initial, land.site_signature(7, Vec3::ZERO, 18.));
    }
}
