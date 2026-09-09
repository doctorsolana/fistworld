//! Append-only residential wards. A ward is a street preference, not permission
//! to build: every proposed shell still passes the ordinary terrain, property,
//! defense and full-width road proofs. Existing houses are never moved.

use bevy::prelude::*;
use shared::components::{SettlementBuildingKind as Kind, SettlementDevelopment, VillageRoad};
use shared::terrain::WorldTerrain;
use shared::worldgen::splitmix64 as mix;

use super::neighborhood::{house_frontage_pitch, PlotNeighbor};
use super::plots::{PlannedPlotCandidate, MAX_SETTLEMENT_SEARCH_RADIUS};
use super::road_access::nearest_completed_road_frontage;

const WARD_HALF_EXTENTS: Vec2 = Vec2::new(48.0, 46.0);

/// Server-owned planning history on the Hall. It changes only when a new ward
/// is surveyed at permit cadence; daily prosperity and population never rotate
/// or resize an established neighborhood.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct SettlementUrbanPlan {
    pub(crate) wards: Vec<ResidentialWard>,
    last_survey: Option<(usize, usize, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResidentialWard {
    pub(crate) id: u16,
    pub(crate) center: Vec2,
    /// Unit direction along the local streets, in world coordinates.
    pub(crate) axis: Vec2,
    pub(crate) half_extents: Vec2,
    pub(crate) seed: u64,
}

impl ResidentialWard {
    pub(crate) fn contains(&self, point: Vec2) -> bool {
        let delta = point - self.center;
        let side = self.side();
        delta.dot(self.axis).abs() <= self.half_extents.x
            && delta.dot(side).abs() <= self.half_extents.y
    }

    fn side(&self) -> Vec2 {
        Vec2::new(-self.axis.y, self.axis.x)
    }

    fn slots(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let pitch = house_frontage_pitch() + (self.seed & 255) as f32 / 255.0 * 0.8;
        let block = 2.0 * 9.5 + house_frontage_pitch() + 2.0;
        // Three short parallel streets, with room for a cross street / green
        // between the two halves. Several small rows form one larger quarter.
        (-1..=1).flat_map(move |lane| {
            [-3, -2, -1, 1, 2, 3].into_iter().flat_map(move |slot| {
                [-1.0, 1.0].into_iter().map(move |verge| {
                    let bend = if lane == 0 {
                        0.0
                    } else {
                        ((self.seed.rotate_right(12) & 255) as f32 / 255.0 - 0.5)
                            * 2.0
                            * slot as f32
                            / 3.0
                    };
                    let frontage = self.center
                        + self.axis * (slot as f32 * pitch)
                        + self.side() * (lane as f32 * block + bend);
                    (frontage + self.side() * verge * 9.5, frontage)
                })
            })
        })
    }
}

fn position_key(point: Vec2) -> u64 {
    u64::from(point.x.to_bits()) ^ u64::from(point.y.to_bits()).rotate_left(31)
}

fn sort_anchors(anchors: &mut [(Vec2, Vec2)], seed: u64) {
    // Opposing homes can share the exact same frontage. Break every tie by
    // geometry so ECS/road iteration order never decides the ward's bearing.
    anchors.sort_by(|(point_a, axis_a), (point_b, axis_b)| {
        mix(seed ^ position_key(*point_a))
            .cmp(&mix(seed ^ position_key(*point_b)))
            .then_with(|| point_a.x.total_cmp(&point_b.x))
            .then_with(|| point_a.y.total_cmp(&point_b.y))
            .then_with(|| axis_a.x.total_cmp(&axis_b.x))
            .then_with(|| axis_a.y.total_cmp(&axis_b.y))
    });
}

fn vacant(point: Vec2, occupied: &[(Vec3, f32)]) -> bool {
    occupied.iter().all(|(other, clearance)| {
        point.distance_squared(Vec2::new(other.x, other.z))
            >= (Kind::House.clearance() + clearance).powi(2)
    })
}

impl SettlementUrbanPlan {
    fn adjoins_existing_ward(&self, center: Vec2, axis: Vec2) -> bool {
        // A street-width seam may separate two blocks. Comparing oriented
        // outlines also admits a seed-selected bend rather than requiring a
        // citywide grid or a shared bearing.
        let seam = Vec2::splat(house_frontage_pitch() * 0.5);
        self.wards.iter().any(|ward| {
            shared::components::oriented_rects_overlap(
                ward.center,
                ward.half_extents + seam,
                -ward.axis.y.atan2(ward.axis.x),
                center,
                WARD_HALF_EXTENTS + seam,
                -axis.y.atan2(axis.x),
            )
        })
    }

    /// Keep the existing bounded survey, but give real block continuations a
    /// guaranteed share of it. Random farm-road samples cannot crowd every
    /// adjacent option out before its terrain/space checks are even attempted.
    fn survey_anchors(&self, mut anchors: Vec<(Vec2, Vec2)>, seed: u64) -> Vec<(Vec2, Vec2)> {
        sort_anchors(&mut anchors, seed);
        if self.wards.is_empty() {
            anchors.truncate(64);
            return anchors;
        }
        let mut continuations: Vec<_> = self
            .wards
            .iter()
            .flat_map(|ward| {
                [
                    ward.axis * ward.half_extents.x * 2.0,
                    -ward.axis * ward.half_extents.x * 2.0,
                    ward.side() * ward.half_extents.y * 2.0,
                    -ward.side() * ward.half_extents.y * 2.0,
                ]
                .into_iter()
                .map(move |offset| (ward.center + offset, ward.axis))
            })
            .collect();
        sort_anchors(&mut continuations, seed);
        continuations.truncate(16);
        let (near, far): (Vec<_>, Vec<_>) = anchors
            .into_iter()
            .partition(|(center, axis)| self.adjoins_existing_ward(*center, *axis));
        continuations.extend(near);
        continuations.truncate(48);
        let remaining = 64 - continuations.len();
        continuations.extend(far.into_iter().take(remaining));
        continuations
    }

    /// Survey at most one additional ward per housing decision. The 18-home
    /// trigger is expansion pressure, not a quota: sites can fill other wards
    /// or the ordinary street grammar, and blocked land can open a branch early.
    pub(super) fn extend_for_housing(
        &mut self,
        terrain: &WorldTerrain,
        charter: &SettlementDevelopment,
        hall: Vec3,
        neighbors: &[PlotNeighbor],
        roads: &[&VillageRoad],
        occupied: &[(Vec3, f32)],
    ) -> bool {
        const MAX_WARDS: usize = 12;
        let homes: Vec<_> = neighbors.iter().filter(|p| p.kind == Kind::House).collect();
        if homes.len() < 6 || self.wards.len() >= MAX_WARDS {
            return false;
        }
        if !self.wards.is_empty()
            && homes.len() < self.wards.len() * 18
            && self.wards.iter().any(|ward| {
                ward.slots()
                    .filter(|(point, _)| vacant(*point, occupied))
                    .take(4)
                    .count()
                    >= 4
            })
        {
            return false;
        }
        let signature = (
            occupied.len(),
            roads.iter().map(|road| road.built_points().len()).sum(),
            terrain.modification_version(),
        );
        if self.last_survey == Some(signature) {
            return false;
        }
        self.last_survey = Some(signature);
        let id = self.wards.len() as u16;
        let seed = mix(charter.plan_seed ^ u64::from(id).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let hall2 = Vec2::new(hall.x, hall.z);
        let mut anchors: Vec<(Vec2, Vec2)> = homes
            .iter()
            .filter_map(|house| {
                let point = Vec2::new(house.position.x, house.position.z);
                let frontage = nearest_completed_road_frontage(point, roads)?;
                Some((
                    frontage,
                    shared::rotation::local_to_world_xz(Vec2::X, house.rotation),
                ))
            })
            .collect();
        // Later quarters follow actual expanding transport: a farm road or
        // an existing suburb can seed a new mixed residential neighborhood.
        if !self.wards.is_empty() {
            let mut segments: Vec<_> = roads
                .iter()
                .filter(|r| r.is_complete())
                .flat_map(|r| r.built_points().windows(2))
                .filter_map(|pair| {
                    let axis = (pair[1] - pair[0]).try_normalize()?;
                    Some(((pair[0] + pair[1]) * 0.5, axis))
                })
                .collect();
            sort_anchors(&mut segments, seed);
            for (point, axis) in segments.into_iter().take(48) {
                let side = Vec2::new(-axis.y, axis.x);
                anchors.extend(
                    [point, point + side * 28.0, point - side * 28.0]
                        .into_iter()
                        .map(|point| (point, axis)),
                );
            }
        }
        let best = self
            .survey_anchors(anchors, seed)
            .into_iter()
            .filter_map(|(center, axis)| {
                if center.distance(hall2) > MAX_SETTLEMENT_SEARCH_RADIUS - 55.0
                    || center.distance(hall2) < 22.0
                    || self
                        .wards
                        .iter()
                        .any(|ward| ward.center.distance(center) < 78.0)
                {
                    return None;
                }
                let ward = ResidentialWard {
                    id,
                    center,
                    axis,
                    half_extents: WARD_HALF_EXTENTS,
                    seed,
                };
                // A preliminary dry-ground/space survey only. We do not call this
                // a buildability proof or abandon the general planner on a miss.
                let mut reachable_frontages = 0;
                let open = ward
                    .slots()
                    .filter(|(point, frontage)| {
                        if !vacant(*point, occupied) {
                            return false;
                        }
                        let position =
                            Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
                        let dry = shared::components::minimum_building_water_clearance(
                            terrain,
                            position,
                            Kind::House,
                            super::plots::rotation_facing_frontage(*point, *frontage),
                        ) >= super::terrain::FREEBOARD
                            && super::terrain::slope_at(terrain, point.x, point.y)
                                <= super::terrain::MAX_BUILD_SLOPE;
                        if dry
                            && reachable_frontages < 4
                            && roads.iter().any(|road| {
                                road.is_complete()
                                    && road.built_points().windows(2).any(|pair| {
                                        super::road_access::closest_point_on_segment(
                                            *frontage, pair[0], pair[1],
                                        )
                                        .distance_squared(*frontage)
                                            <= 48.0 * 48.0
                                    })
                            })
                        {
                            reachable_frontages += 1;
                        }
                        dry
                    })
                    .count();
                if open < 8 || reachable_frontages < 4 {
                    return None;
                }
                let nearby_homes = homes
                    .iter()
                    .filter(|house| ward.contains(Vec2::new(house.position.x, house.position.z)))
                    .count();
                let score = open as f32 + nearby_homes.min(12) as f32 * 2.0
                    - center.distance(hall2) * 0.075;
                // A viable adjoining block wins before raw vacant acreage.
                // Remote road-served ground remains the fallback when every
                // adjacent option fails the same preliminary physical checks.
                Some((self.adjoins_existing_ward(center, axis), score, ward))
            })
            .max_by(|(adjoins_a, score_a, a), (adjoins_b, score_b, b)| {
                adjoins_a
                    .cmp(adjoins_b)
                    .then_with(|| score_a.total_cmp(score_b))
                    .then_with(|| b.center.x.total_cmp(&a.center.x))
                    .then_with(|| b.center.y.total_cmp(&a.center.y))
                    .then_with(|| b.axis.x.total_cmp(&a.axis.x))
                    .then_with(|| b.axis.y.total_cmp(&a.axis.y))
            });
        if let Some((_, _, ward)) = best {
            self.wards.push(ward);
        }
        true
    }

    /// Return a bounded shortlist of infill and new frontages; its 48 m road
    /// catchment prevents speculative neighborhoods disconnected from town.
    pub(super) fn candidates(
        &self,
        hall: Vec3,
        occupied: &[(Vec3, f32)],
        roads: &[&VillageRoad],
    ) -> Vec<PlannedPlotCandidate> {
        let hall2 = Vec2::new(hall.x, hall.z);
        let mut ranked: Vec<_> = self
            .wards
            .iter()
            .flat_map(|ward| {
                ward.slots().filter_map(move |(point, frontage)| {
                    if !vacant(point, occupied) {
                        return None;
                    }
                    let road = nearest_completed_road_frontage(frontage, roads)?;
                    let extension = frontage.distance(road);
                    // A free spur far from the core should not beat a modest
                    // extension into a vacant inner block. This is a ranking
                    // cost only: all road and property proofs remain unchanged.
                    let score = extension
                        + point.distance(ward.center) * 0.08
                        + point.distance(hall2) * 0.15
                        + f32::from(ward.id) * 0.5;
                    Some((
                        score,
                        PlannedPlotCandidate {
                            local: point - hall2,
                            frontage: frontage - hall2,
                        },
                    ))
                })
            })
            .collect();
        ranked.sort_by(|(sa, a), (sb, b)| {
            sa.total_cmp(sb)
                .then_with(|| a.local.x.total_cmp(&b.local.x))
                .then_with(|| a.local.y.total_cmp(&b.local.y))
        });
        ranked
            .into_iter()
            .take(48)
            .map(|(_, candidate)| candidate)
            .collect()
    }

    /// Soft mixed-use preference. Resource quality has a 100-point range, so
    /// this bounded score never overrules a materially better forest or soil.
    pub(super) fn land_use_score(&self, kind: Kind, point: Vec2) -> f32 {
        let nearest = self
            .wards
            .iter()
            .map(|ward| point.distance(ward.center))
            .min_by(f32::total_cmp);
        let Some(distance) = nearest else {
            return 0.0;
        };
        match kind {
            Kind::Farmstead | Kind::LivestockFarm | Kind::StoneQuarry | Kind::LumberjackHut => {
                -8.0 * (1.0 - distance / 68.0).clamp(0.0, 1.0)
            }
            Kind::Bakery | Kind::Tavern | Kind::Market => {
                6.0 * (1.0 - distance / 90.0).clamp(0.0, 1.0)
            }
            Kind::StorageHall | Kind::Windmill => {
                5.0 * (1.0 - (distance - 58.0).abs() / 58.0).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_street(points: Vec<Vec2>) -> VillageRoad {
        VillageRoad {
            settlement: "Wards".into(),
            builder: "Mara".into(),
            built_through: points.len() as u16,
            points,
            width: 2.0,
            reserved_width: 3.0,
            surface: default(),
            class: default(),
            stone_committed: 0,
        }
    }

    #[test]
    fn quarter_contains_several_safe_streets_and_preserves_a_cross_street() {
        for seed in 0..20 {
            let ward = ResidentialWard {
                id: 0,
                center: Vec2::ZERO,
                axis: Vec2::X,
                half_extents: Vec2::new(48.0, 46.0),
                seed: mix(seed),
            };
            let slots: Vec<_> = ward.slots().collect();
            assert_eq!(slots.len(), 36);
            for (i, (point, _)) in slots.iter().enumerate() {
                assert!(ward.contains(*point));
                assert!(point.x.abs() >= house_frontage_pitch());
                assert!(slots[..i]
                    .iter()
                    .all(|(other, _)| point.distance(*other) >= Kind::House.clearance() * 2.0));
            }
        }
    }

    #[test]
    fn local_businesses_mix_in_while_bulk_land_uses_prefer_the_edge() {
        let plan = SettlementUrbanPlan {
            wards: vec![ResidentialWard {
                id: 0,
                center: Vec2::ZERO,
                axis: Vec2::X,
                half_extents: Vec2::new(48.0, 46.0),
                seed: 7,
            }],
            ..default()
        };
        assert!(
            plan.land_use_score(Kind::Bakery, Vec2::ZERO)
                > plan.land_use_score(Kind::Bakery, Vec2::X * 100.0)
        );
        assert!(
            plan.land_use_score(Kind::Farmstead, Vec2::ZERO)
                < plan.land_use_score(Kind::Farmstead, Vec2::X * 100.0)
        );
        assert!(
            plan.land_use_score(Kind::StorageHall, Vec2::X * 58.0)
                > plan.land_use_score(Kind::StorageHall, Vec2::ZERO)
        );
        assert!(plan.land_use_score(Kind::Farmstead, Vec2::ZERO).abs() <= 8.0);
    }

    #[test]
    fn adjacent_serviced_ward_beats_distant_open_land_with_a_blocked_land_fallback() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let charter = SettlementDevelopment::from_seed(23, 0);
        let original = ResidentialWard {
            id: 0,
            center: hall.xz() + Vec2::new(-30.0, -40.0),
            axis: Vec2::X,
            half_extents: WARD_HALF_EXTENTS,
            seed: 7,
        };
        let homes: Vec<_> = original
            .slots()
            .take(18)
            .map(|(point, frontage)| PlotNeighbor {
                kind: Kind::House,
                position: Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y),
                rotation: super::super::plots::rotation_facing_frontage(point, frontage),
            })
            .collect();
        // Two real completed links expose both the edge of the existing block
        // and a pristine serviced pocket over 200 m away from that block.
        let roads = [
            completed_street(vec![
                original.center - Vec2::X * 45.0,
                original.center + Vec2::X * 145.0,
            ]),
            completed_street(vec![
                original.center + Vec2::X * 145.0,
                original.center + Vec2::X * 265.0,
            ]),
        ];
        let road_refs = [&roads[0], &roads[1]];
        for inner_blocked in [false, true] {
            let mut plan = SettlementUrbanPlan {
                wards: vec![original.clone()],
                ..default()
            };
            let mut occupied: Vec<_> = homes
                .iter()
                .map(|house| (house.position, Kind::House.clearance()))
                .collect();
            occupied.push((hall, Kind::Hall.clearance()));
            if inner_blocked {
                // Coarse claimed-land snapshot: all adjoining blocks are
                // unavailable, but the distant end of the public road is open.
                occupied.push((Vec3::new(original.center.x, 0.0, original.center.y), 180.0));
            }
            assert!(
                plan.extend_for_housing(&terrain, &charter, hall, &homes, &road_refs, &occupied)
            );
            assert_eq!(
                plan.wards.len(),
                2,
                "a viable serviced ward remains available, blocked={inner_blocked}"
            );
            assert_eq!(plan.wards[0], original, "accepted geometry must never move");
            let next = &plan.wards[1];
            let old_plan = SettlementUrbanPlan {
                wards: vec![original.clone()],
                ..default()
            };
            assert_eq!(old_plan.adjoins_existing_ward(next.center, next.axis), !inner_blocked,
                "prefer contiguous housing; use distant serviced land only if adjacent land is blocked: next={next:?}");
        }
    }

    #[test]
    fn opposing_frontage_anchors_have_a_stable_order() {
        let mut original = vec![
            (Vec2::ZERO, Vec2::X),
            (Vec2::ZERO, Vec2::NEG_X),
            (Vec2::X, Vec2::Y),
            (Vec2::ZERO, Vec2::Y),
        ];
        let mut reversed = original.iter().copied().rev().collect::<Vec<_>>();
        sort_anchors(&mut original, 23);
        sort_anchors(&mut reversed, 23);
        assert_eq!(original, reversed);
    }

    #[test]
    fn residential_growth_retains_old_wards_and_certifies_every_new_connector() {
        use super::super::plots::find_site_with_plan_diagnostics;
        use super::super::road_access::{planned_road_access_path, road_access_blockers_for_plot};
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let charter = SettlementDevelopment::from_seed(23, 0);
        let mut plan = SettlementUrbanPlan::default();
        let mut neighbors = Vec::new();
        let mut occupied = vec![(hall, Kind::Hall.clearance())];
        let mut roads: Vec<VillageRoad> = Vec::new();
        let mut blockers = Vec::new();
        let mut in_quarters = 0;
        for number in 0..48 {
            let previous_wards = plan.wards.clone();
            let road_refs: Vec<_> = roads.iter().collect();
            plan.extend_for_housing(&terrain, &charter, hall, &neighbors, &road_refs, &occupied);
            assert_eq!(&plan.wards[..previous_wards.len()], &previous_wards);
            let (position, rotation) = find_site_with_plan_diagnostics(
                &terrain,
                hall,
                Kind::House,
                &occupied,
                &neighbors,
                &road_refs,
                &[],
                &blockers,
                Some(&charter),
                None,
                None,
                None,
                None,
                None,
                Some(&plan),
                None,
                &[],
            )
            .unwrap_or_else(|| panic!("house {number} has no legal plot"));
            assert!(occupied
                .iter()
                .all(|(other, radius)| position.xz().distance(other.xz())
                    >= Kind::House.clearance() + radius));
            let connected = crate::world::village_roads::hall_connected_road_keys(
                Kind::Hall.entrance_position(hall, 0.0).xz(),
                &road_refs,
            );
            let access = planned_road_access_path(
                &terrain,
                hall,
                Kind::House,
                position,
                rotation,
                &road_refs,
                &blockers,
                &connected,
            )
            .expect("accepted house has its certified connector");
            assert!(access.len() >= 2);
            // This is a placement regression, not economic growth: it marks the
            // certified connector complete before asking for the next plot.
            roads.push(completed_street(access));
            in_quarters += usize::from(plan.wards.iter().any(|ward| ward.contains(position.xz())));
            neighbors.push(PlotNeighbor {
                kind: Kind::House,
                position,
                rotation,
            });
            occupied.push((position, Kind::House.clearance()));
            blockers.extend(road_access_blockers_for_plot(
                Kind::House,
                position,
                rotation,
            ));
        }
        assert!(
            plan.wards.len() >= 2,
            "48 houses need more than the founding quarter"
        );
        assert!(
            in_quarters >= 24,
            "quarters should shape actual accepted houses, got {in_quarters}"
        );
        let refs: Vec<_> = roads.iter().collect();
        let connected = crate::world::village_roads::hall_connected_road_keys(
            Kind::Hall.entrance_position(hall, 0.0).xz(),
            &refs,
        );
        assert!(roads.iter().all(|road| {
            road.points.iter().any(|point| {
                connected.contains(&crate::world::village_roads::road_point_key(*point))
            })
        }));
    }
}
