//! Bounded, soft relationships between plots. These are candidate preferences;
//! the ordinary terrain, reservation and full-width access proofs still decide
//! whether any of them can become a permit.

use bevy::prelude::*;
use shared::components::{SettlementBuildingKind as Kind, SettlementDevelopment, VillageRoad};
use shared::worldgen::splitmix64 as mix;

use super::plots::PlannedPlotCandidate;
use super::road_access::nearest_completed_road_frontage;

#[derive(Clone, Copy, Debug)]
pub(super) struct PlotNeighbor {
    pub(super) kind: Kind,
    pub(super) position: Vec3,
    pub(super) rotation: f32,
}

/// A frontage pitch must fit both upgraded house lines, as well as the coarse
/// plot reservation. The former 11 m grid pitch conflicted with the 12 m house
/// exclusion and skipped every immediate neighbour on an otherwise empty row.
pub(super) fn house_frontage_pitch() -> f32 {
    let footprint = Kind::House.placement_definition();
    (Kind::House.clearance() * 2.0).max(footprint.footprint.x + 2.0) + 0.2
}

fn place_key(position: Vec3) -> u64 {
    u64::from(position.x.to_bits()) ^ u64::from(position.z.to_bits()).rotate_left(31)
}

/// Return at most 24 adjacent plots, drawn from at most 12 existing/approved
/// homes. Anchor sampling rotates deterministically as the town grows, so a
/// filled founding street cannot monopolise every future infill attempt.
pub(super) fn house_frontage_candidates(
    plan: &SettlementDevelopment,
    hall: Vec3,
    neighbors: &[PlotNeighbor],
    roads: &[&VillageRoad],
    min_radius: f32,
    max_radius: f32,
) -> Vec<PlannedPlotCandidate> {
    const MAX_ANCHORS: usize = 12;
    const MAX_GROUP_SIZE: usize = 5;
    let mut anchors: Vec<_> = neighbors
        .iter()
        .filter(|neighbor| neighbor.kind == Kind::House)
        .collect();
    anchors.sort_by(|a, b| {
        mix(plan.plan_seed ^ place_key(a.position))
            .cmp(&mix(plan.plan_seed ^ place_key(b.position)))
            .then_with(|| a.position.x.total_cmp(&b.position.x))
            .then_with(|| a.position.z.total_cmp(&b.position.z))
    });
    if anchors.is_empty() {
        return Vec::new();
    }
    let offset = neighbors.len() % anchors.len();
    anchors.rotate_left(offset);
    let hall2 = Vec2::new(hall.x, hall.z);
    let pitch = house_frontage_pitch();
    let mut candidates = Vec::with_capacity(MAX_ANCHORS * 2);
    for anchor in anchors.into_iter().take(MAX_ANCHORS) {
        let anchor2 = Vec2::new(anchor.position.x, anchor.position.z);
        let Some(frontage) = nearest_completed_road_frontage(anchor2, roads) else {
            continue;
        };
        // A roadside group retains the existing facade direction. Frontage may
        // extend a short lane beyond its end, but the permit must reserve and
        // certify the new connector before the house is approved.
        let along = shared::rotation::local_to_world_xz(Vec2::X, anchor.rotation);
        let outward = shared::rotation::local_to_world_xz(Vec2::Y, anchor.rotation);
        let facing_dot = outward.dot((anchor2 - frontage).normalize_or_zero());
        if facing_dot < 0.8 || anchor2.distance(frontage) > 24.0 {
            continue;
        }
        let row: Vec<_> = neighbors
            .iter()
            .filter(|other| {
                if other.kind != Kind::House {
                    return false;
                }
                let delta = Vec2::new(other.position.x, other.position.z) - anchor2;
                delta.dot(outward).abs() < 3.0
                    && delta.dot(along).abs() < pitch * MAX_GROUP_SIZE as f32
                    && (other.rotation - anchor.rotation).cos() > 0.94
            })
            .collect();
        // Adjacent houses share a stable geometric anchor's temperament.
        // It is a soft group size, not a zoning prohibition: the normal grammar
        // can still use these streets when other viable plots are scarce.
        let row_seed = row
            .iter()
            .map(|neighbor| place_key(neighbor.position))
            .min()
            .unwrap_or_else(|| place_key(anchor.position));
        let desired_size = 2 + (mix(plan.plan_seed ^ row_seed) % 4) as usize;
        if row.len() >= desired_size {
            continue;
        }
        let extra_yard =
            (mix(plan.plan_seed ^ row_seed.rotate_left(13)) & 255) as f32 / 255.0 * 1.2;
        for sign in [-1.0, 1.0] {
            let shift = along * sign * (pitch + extra_yard);
            let position = anchor2 + shift;
            let radius = position.distance(hall2);
            if radius < min_radius || radius > max_radius {
                continue;
            }
            // Do not spend a route proof on an already occupied neighbour.
            // Pending buildings are included in `neighbors` as well.
            if neighbors.iter().any(|other| {
                position.distance_squared(Vec2::new(other.position.x, other.position.z))
                    < (Kind::House.clearance() + other.kind.clearance()).powi(2)
            }) {
                continue;
            }
            let target = frontage + shift;
            let Some(existing_frontage) = nearest_completed_road_frontage(position, roads) else {
                continue;
            };
            // Permit one house-width of street extension, not a second road
            // system inferred through distant fields or across the settlement.
            let extension = existing_frontage.distance(target);
            if extension > pitch + 2.0 {
                continue;
            }
            let score = 20.0 - extension * 0.5 - radius * 0.025;
            candidates.push((
                score,
                PlannedPlotCandidate {
                    local: position - hall2,
                    frontage: target - hall2,
                },
            ));
        }
    }
    candidates.sort_by(|(score_a, a), (score_b, b)| {
        score_b
            .total_cmp(score_a)
            .then_with(|| a.local.x.total_cmp(&b.local.x))
            .then_with(|| a.local.y.total_cmp(&b.local.y))
    });
    candidates.dedup_by(|(_, a), (_, b)| a.local.distance_squared(b.local) < 0.01);
    candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

/// A modest location preference, never an economic output multiplier or a hard
/// district boundary. Nearest compatible work wins; adding many copies cannot
/// overwhelm soil, dry access or the physical constraints with a larger sum.
pub(super) fn affinity_score(kind: Kind, position: Vec2, neighbors: &[PlotNeighbor]) -> f32 {
    neighbors.iter().fold(0.0_f32, |best, neighbor| {
        let compatible = match kind {
            Kind::Farmstead | Kind::LivestockFarm => {
                matches!(neighbor.kind, Kind::Farmstead | Kind::LivestockFarm)
            }
            Kind::Windmill => neighbor.kind == Kind::Farmstead,
            Kind::Bakery => matches!(neighbor.kind, Kind::Windmill | Kind::Market),
            Kind::LumberjackHut | Kind::StoneQuarry => {
                matches!(
                    neighbor.kind,
                    Kind::LumberjackHut | Kind::StoneQuarry | Kind::StorageHall
                )
            }
            Kind::StorageHall => matches!(
                neighbor.kind,
                Kind::Farmstead
                    | Kind::LivestockFarm
                    | Kind::LumberjackHut
                    | Kind::StoneQuarry
                    | Kind::Windmill
                    | Kind::Bakery
                    | Kind::Market
            ),
            _ => false,
        };
        if !compatible {
            return best;
        }
        let distance = position.distance(Vec2::new(neighbor.position.x, neighbor.position.z));
        best.max((1.0 - distance / 72.0).clamp(0.0, 1.0) * 7.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street(start: f32, end: f32) -> VillageRoad {
        VillageRoad {
            settlement: "Frontage".into(),
            builder: "Mara".into(),
            points: vec![Vec2::new(start, 0.0), Vec2::new(end, 0.0)],
            built_through: 2,
            width: 2.0,
            reserved_width: 3.0,
            surface: default(),
            class: default(),
            stone_committed: 0,
        }
    }

    #[test]
    fn seeded_frontage_forms_small_safe_rows_and_stops_extending_them() {
        let road = street(-120.0, 300.0);
        let mut sizes = std::collections::BTreeSet::new();
        for seed in 0..24 {
            let mut plan = SettlementDevelopment::from_foundation("Frontage", Vec3::ZERO, 0);
            plan.plan_seed = seed;
            let mut neighbors = vec![PlotNeighbor {
                kind: Kind::House,
                position: Vec3::new(80.0, 0.0, 12.0),
                rotation: 0.0,
            }];
            for _ in 0..6 {
                let candidates =
                    house_frontage_candidates(&plan, Vec3::ZERO, &neighbors, &[&road], 0.0, 300.0);
                let Some(candidate) = candidates.first() else {
                    break;
                };
                assert!(candidates.len() <= 24);
                assert!(
                    (candidate.local.y - 12.0).abs() < 0.001,
                    "new homes retain their street's setback"
                );
                assert!(
                    candidate.frontage.y.abs() < 0.001,
                    "doors retain a common street frontage"
                );
                assert!(
                    neighbors.iter().all(|neighbor| {
                        candidate
                            .local
                            .distance(Vec2::new(neighbor.position.x, neighbor.position.z))
                            >= Kind::House.clearance() * 2.0
                    }),
                    "same-street infill must leave room for both upgraded houses"
                );
                neighbors.push(PlotNeighbor {
                    kind: Kind::House,
                    position: Vec3::new(candidate.local.x, 0.0, candidate.local.y),
                    rotation: 0.0,
                });
            }
            assert!(
                (2..=5).contains(&neighbors.len()),
                "seed {seed} created an unbounded row: {}",
                neighbors.len()
            );
            sizes.insert(neighbors.len());
        }
        assert!(
            sizes.len() >= 3,
            "seeds should vary local group size, not just rotate one plan"
        );
    }

    #[test]
    fn frontage_choice_is_independent_of_neighbor_iteration_order() {
        let plan = SettlementDevelopment::from_foundation("Frontage", Vec3::ZERO, 0);
        let road = street(-120.0, 300.0);
        let neighbors: Vec<_> = [30.0, 110.0, 190.0]
            .into_iter()
            .map(|x| PlotNeighbor {
                kind: Kind::House,
                position: Vec3::new(x, 0.0, 12.0),
                rotation: 0.0,
            })
            .collect();
        let read = |neighbors: &[PlotNeighbor]| {
            house_frontage_candidates(&plan, Vec3::ZERO, neighbors, &[&road], 0.0, 300.0)
                .into_iter()
                .map(|candidate| (candidate.local, candidate.frontage))
                .collect::<Vec<_>>()
        };
        let expected = read(&neighbors);
        assert!(!expected.is_empty());
        let mut reversed = neighbors.clone();
        reversed.reverse();
        assert_eq!(expected, read(&reversed));
        assert!(
            house_frontage_candidates(&plan, Vec3::ZERO, &neighbors, &[], 0.0, 300.0).is_empty(),
            "implied infill does not manufacture completed roads"
        );
    }

    #[test]
    fn equidistant_street_frontage_does_not_depend_on_road_iteration_order() {
        let lower = street(-120.0, 300.0);
        let mut upper = lower.clone();
        for point in &mut upper.points {
            point.y = 24.0;
        }
        let candidate = Vec2::new(80.0, 12.0);
        assert_eq!(
            nearest_completed_road_frontage(candidate, &[&lower, &upper]),
            nearest_completed_road_frontage(candidate, &[&upper, &lower]),
        );
    }

    #[test]
    fn frontage_pitch_fits_both_upgrade_lines_and_plot_reservations() {
        let pitch = house_frontage_pitch();
        assert!(pitch > Kind::House.clearance() * 2.0);
        for building in [
            shared::building::BuildingType::CabinL2,
            shared::building::BuildingType::LongCabinL2,
        ] {
            assert!(pitch >= building.definition().footprint.x + 2.0);
        }
    }

    #[test]
    fn affinity_rewards_compatible_nearby_work_without_rewarding_repeated_copies() {
        let farm = PlotNeighbor {
            kind: Kind::Farmstead,
            position: Vec3::new(24.0, 0.0, 0.0),
            rotation: 0.0,
        };
        let unrelated = PlotNeighbor {
            kind: Kind::House,
            ..farm
        };
        let near = affinity_score(Kind::Windmill, Vec2::ZERO, &[farm]);
        assert!(near > affinity_score(Kind::Windmill, Vec2::new(-60.0, 0.0), &[farm]));
        assert!(near > affinity_score(Kind::Windmill, Vec2::ZERO, &[unrelated]));
        assert_eq!(
            near,
            affinity_score(Kind::Windmill, Vec2::ZERO, &[farm; 100])
        );
        assert!(
            near < 7.0,
            "soft proximity must not outweigh a large soil-quality difference"
        );
    }
}
